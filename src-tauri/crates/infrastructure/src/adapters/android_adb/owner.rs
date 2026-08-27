use std::{collections::BTreeMap, future::Future, sync::Arc};

use chrono::Utc;
use intercept_proxy_application::{
    AndroidNetworkState, AndroidRuntimeOwnerMode, AndroidRuntimeOwnerState,
    AndroidRuntimeOwnerTransitionReason, AndroidRuntimeOwnerViewModel, AppError, AppResult,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::environment_apply::publish_android_owner_mutation;
use super::{AndroidAdbAdapter, AndroidOwnerState, PreparedUsbProxyRuntime};
use crate::{AndroidRuntimeOwnerRecord, SqliteExecutor};

mod persistence;
mod replacement;
use persistence::{owner_store_error, owns_epoch, persist_replace, publish_record, reverse_ports};

#[derive(Clone)]
pub(super) struct OwnerTransitionContext {
    pub(super) serial: String,
    pub(super) states: Arc<Mutex<BTreeMap<String, AndroidOwnerState>>>,
    pub(super) executor: SqliteExecutor,
}

impl OwnerTransitionContext {
    pub(super) async fn snapshot(&self) -> AndroidOwnerState {
        self.states
            .lock()
            .await
            .get(&self.serial)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) async fn update(&self, update: impl FnOnce(&mut AndroidOwnerState)) {
        let mut states = self.states.lock().await;
        update(states.entry(self.serial.clone()).or_default());
        if states
            .get(&self.serial)
            .is_some_and(AndroidOwnerState::is_empty)
        {
            states.remove(&self.serial);
        }
    }

    async fn runtime_owner_conflict_error(&self) -> AppError {
        authoritative_owner_conflict_error(&self.executor, &self.serial).await
    }
}

impl AndroidAdbAdapter {
    async fn run_owner_transition<T, F, Fut>(&self, serial: &str, operation: F) -> AppResult<T>
    where
        T: Send,
        F: FnOnce(OwnerTransitionContext) -> Fut + Send,
        Fut: Future<Output = AppResult<T>> + Send,
    {
        let context = OwnerTransitionContext {
            serial: serial.to_owned(),
            states: Arc::clone(&self.owner_states),
            executor: self.sqlite_executor.clone(),
        };
        let before = context.snapshot().await.runtime_owner;
        let result = operation(context.clone()).await;
        if result.is_ok() {
            let after = context.snapshot().await.runtime_owner;
            if before != after {
                publish_android_owner_mutation(
                    &self.environment_apply_resource_gates,
                    before.as_ref(),
                    after.as_ref(),
                );
            }
        }
        result
    }

    pub(super) async fn runtime_owner_snapshot_for(
        &self,
        serial: &str,
    ) -> Option<AndroidRuntimeOwnerViewModel> {
        self.owner_state_snapshot_for(serial).await.runtime_owner
    }

    pub(super) async fn runtime_owner_snapshots(&self) -> Vec<AndroidRuntimeOwnerViewModel> {
        self.owner_states
            .lock()
            .await
            .values()
            .filter_map(|state| state.runtime_owner.clone())
            .collect()
    }

    pub(super) async fn authoritative_runtime_owners(
        &self,
    ) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        self.sqlite_executor
            .execute(|store| {
                store
                    .load_android_runtime_owners()
                    .map(|records| records.into_iter().map(|record| record.owner).collect())
            })
            .await
            .map_err(|error| {
                AppError::new(
                    "ANDROID_RUNTIME_OWNER_PERSISTENCE_FAILED",
                    format!("Android 运行设备记录读取失败：{error}"),
                )
            })
    }

    pub(super) async fn contextualize_authoritative_owner_error(
        &self,
        serial: &str,
        error: AppError,
    ) -> AppError {
        let fallback_epoch = error.view_model.runtime_epoch;
        match self.authoritative_runtime_owners().await {
            Ok(owners) => error.runtime_context(
                serial,
                owners
                    .into_iter()
                    .find(|owner| owner.serial == serial)
                    .map(|owner| owner.epoch),
            ),
            Err(_) => error.runtime_context(serial, fallback_epoch),
        }
    }

    pub(super) async fn owner_state_snapshot_for(&self, serial: &str) -> AndroidOwnerState {
        self.owner_states
            .lock()
            .await
            .get(serial)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) async fn required_runtime_target(
        &self,
        serial: &str,
        expected_epoch: Uuid,
    ) -> AppResult<AndroidRuntimeOwnerViewModel> {
        match self.runtime_owner_snapshot_for(serial).await {
            Some(owner) if owner.epoch == expected_epoch => Ok(owner),
            Some(owner) => Err(stale_owner_error(serial, owner.epoch)),
            None => Err(not_managed_error(serial)),
        }
    }

    pub(super) async fn runtime_owner_conflict_error(&self, serial: &str) -> AppError {
        authoritative_owner_conflict_error(&self.sqlite_executor, serial).await
    }

    pub(super) async fn ensure_can_start(&self, serial: &str) -> AppResult<()> {
        if let Some(owner) = self.runtime_owner_snapshot_for(serial).await {
            return Err(AppError::new(
                "ANDROID_RUNTIME_ALREADY_MANAGED",
                format!("设备 {serial} 已有登记中的网络运行态。"),
            )
            .entity(serial)
            .epoch(owner.epoch)
            .retryable("请先停止或恢复该设备，再重新启动。"));
        }
        Ok(())
    }

    pub(super) async fn publish_prepared_owner(
        &self,
        prepared: &PreparedUsbProxyRuntime,
        state: AndroidRuntimeOwnerState,
        reason: AndroidRuntimeOwnerTransitionReason,
    ) -> AppResult<()> {
        let mut owner = prepared.owner.clone();
        owner.state = state;
        owner.transition_reason = reason;
        owner.updated_at = Utc::now();
        let serial = owner.serial.clone();
        let transition_serial = serial.clone();
        let runtime = prepared.runtime.clone();
        self.run_owner_transition(&transition_serial, move |context| async move {
            let state = context.snapshot().await;
            let record = AndroidRuntimeOwnerRecord {
                reverse_ports: reverse_ports(&state, owner.epoch),
                resume_state: state.runtime_resume_state,
                runtime_endpoints: state.runtime_endpoints,
                owner: owner.clone(),
            };
            let record_for_store = record.clone();
            let serial_for_store = serial.clone();
            let error_serial = serial.clone();
            let current_epoch = record.owner.epoch;
            let replaced = context
                .executor
                .execute(move |store| {
                    store.replace_android_runtime_owner_if_epoch(
                        &serial_for_store,
                        record_for_store.owner.epoch,
                        &record_for_store,
                    )
                })
                .await
                .map_err(|error| owner_store_error(&error, &error_serial, Some(current_epoch)))?;
            if !replaced {
                return Err(context.runtime_owner_conflict_error().await);
            }
            context
                .update(|state| {
                    state.runtime_owner = Some(owner);
                    state.active_runtime = Some(runtime);
                })
                .await;
            Ok(())
        })
        .await
    }

    pub(super) async fn stage_prepared_cleanup(
        &self,
        prepared: &PreparedUsbProxyRuntime,
        cleanup_ports: Vec<u16>,
        expected_epoch: Option<Uuid>,
    ) -> AppResult<()> {
        let mut owner = prepared.owner.clone();
        owner.state = AndroidRuntimeOwnerState::CleanupRequired;
        owner.transition_reason = AndroidRuntimeOwnerTransitionReason::ReversePreparation;
        owner.updated_at = Utc::now();
        let serial = owner.serial.clone();
        let transition_serial = serial.clone();
        let endpoints = prepared.runtime.endpoints.clone();
        self.run_owner_transition(&transition_serial, move |context| async move {
            let record = AndroidRuntimeOwnerRecord {
                owner: owner.clone(),
                reverse_ports: cleanup_ports.clone(),
                resume_state: None,
                runtime_endpoints: endpoints.clone(),
            };
            let serial_for_store = serial.clone();
            let error_serial = serial.clone();
            let reserved = context
                .executor
                .execute(move |store| match expected_epoch {
                    Some(expected_epoch) => store.replace_android_runtime_owner_if_epoch(
                        &serial_for_store,
                        expected_epoch,
                        &record,
                    ),
                    None => store.reserve_android_runtime_owner(&record).map(|()| true),
                })
                .await
                .map_err(|error| owner_store_error(&error, &error_serial, expected_epoch))?;
            if !reserved {
                return Err(context.runtime_owner_conflict_error().await);
            }
            context
                .update(|state| publish_record(state, owner, cleanup_ports, None, endpoints))
                .await;
            Ok(())
        })
        .await
    }

    #[cfg(test)]
    pub(super) async fn save_owner(&self, owner: AndroidRuntimeOwnerViewModel) -> AppResult<()> {
        let serial = owner.serial.clone();
        let transition_serial = serial.clone();
        self.run_owner_transition(&transition_serial, move |context| async move {
            let state = context.snapshot().await;
            let record = AndroidRuntimeOwnerRecord {
                reverse_ports: reverse_ports(&state, owner.epoch),
                resume_state: state.runtime_resume_state,
                runtime_endpoints: state.runtime_endpoints,
                owner: owner.clone(),
            };
            context
                .executor
                .execute(move |store| store.reserve_android_runtime_owner(&record))
                .await
                .map_err(|error| owner_store_error(&error, &serial, None))?;
            context
                .update(|state| state.runtime_owner = Some(owner))
                .await;
            Ok(())
        })
        .await
    }

    pub(super) async fn mark_owner_waiting_reconnect(
        &self,
        serial: &str,
        expected_epoch: Uuid,
    ) -> AppResult<()> {
        self.update_owner(serial, expected_epoch, move |mut owner, state| {
            let resume_state = if owner.state == AndroidRuntimeOwnerState::WaitingReconnect {
                state.runtime_resume_state
            } else {
                Some(owner.state)
            };
            owner.state = AndroidRuntimeOwnerState::WaitingReconnect;
            owner.transition_reason = AndroidRuntimeOwnerTransitionReason::DeviceDisconnected;
            owner.updated_at = Utc::now();
            (owner, resume_state)
        })
        .await
    }

    pub(super) async fn mark_owner_reconnected(
        &self,
        serial: &str,
        expected_epoch: Uuid,
        observed: Option<AndroidNetworkState>,
    ) -> AppResult<()> {
        self.update_owner(serial, expected_epoch, move |mut owner, state| {
            if owner.state != AndroidRuntimeOwnerState::WaitingReconnect {
                return (owner, state.runtime_resume_state);
            }
            owner.state = match state.runtime_resume_state {
                Some(AndroidRuntimeOwnerState::CleanupRequired) => {
                    AndroidRuntimeOwnerState::CleanupRequired
                }
                Some(AndroidRuntimeOwnerState::StopFailed) => AndroidRuntimeOwnerState::StopFailed,
                _ => match observed {
                    Some(AndroidNetworkState::Running) => AndroidRuntimeOwnerState::Active,
                    Some(AndroidNetworkState::Stopped | AndroidNetworkState::Faulted) => {
                        AndroidRuntimeOwnerState::CleanupRequired
                    }
                    _ => AndroidRuntimeOwnerState::Uncertain,
                },
            };
            owner.transition_reason = AndroidRuntimeOwnerTransitionReason::DeviceReconnected;
            owner.updated_at = Utc::now();
            (owner, None)
        })
        .await
    }

    pub(super) async fn mark_owner_stop_failed(
        &self,
        serial: &str,
        expected_epoch: Uuid,
        _reason: String,
    ) -> AppResult<()> {
        self.update_owner(serial, expected_epoch, move |mut owner, _| {
            owner.state = AndroidRuntimeOwnerState::StopFailed;
            owner.transition_reason = AndroidRuntimeOwnerTransitionReason::StopFailed;
            owner.updated_at = Utc::now();
            (owner, None)
        })
        .await
    }

    async fn update_owner<F>(&self, serial: &str, expected_epoch: Uuid, update: F) -> AppResult<()>
    where
        F: FnOnce(
                AndroidRuntimeOwnerViewModel,
                &AndroidOwnerState,
            ) -> (
                AndroidRuntimeOwnerViewModel,
                Option<AndroidRuntimeOwnerState>,
            ) + Send,
    {
        self.run_owner_transition(serial, move |context| async move {
            let state = context.snapshot().await;
            let owner = match state.runtime_owner.clone() {
                Some(owner) if owner.epoch == expected_epoch => owner,
                Some(owner) => return Err(stale_owner_error(serial, owner.epoch)),
                None => return Err(not_managed_error(serial)),
            };
            let (owner, resume_state) = update(owner, &state);
            let ports = reverse_ports(&state, expected_epoch);
            let endpoints = state.runtime_endpoints;
            if !persist_replace(&context, owner, ports, resume_state, endpoints, None).await? {
                return Err(context.runtime_owner_conflict_error().await);
            }
            Ok(())
        })
        .await
    }

    pub(super) async fn clear_owner_if_epoch_under_gate(
        &self,
        serial: &str,
        expected_epoch: Uuid,
    ) -> AppResult<bool> {
        self.run_owner_transition(serial, move |context| async move {
            if !owns_epoch(&context.snapshot().await, expected_epoch) {
                return Ok(false);
            }
            let serial_for_store = context.serial.clone();
            let cleared = context
                .executor
                .execute(move |store| {
                    store.clear_android_runtime_owner(&serial_for_store, expected_epoch)
                })
                .await
                .map_err(|error| owner_store_error(&error, serial, Some(expected_epoch)))?;
            if cleared {
                context
                    .update(|state| {
                        if owns_epoch(state, expected_epoch) {
                            *state = AndroidOwnerState::default();
                        }
                    })
                    .await;
            }
            Ok(cleared)
        })
        .await
    }
}

impl AndroidOwnerState {
    fn is_empty(&self) -> bool {
        self.active_reverse.is_none()
            && self.active_runtime.is_none()
            && self.runtime_endpoints.is_empty()
            && self.runtime_owner.is_none()
            && self.runtime_resume_state.is_none()
    }
}

fn not_managed_error(serial: &str) -> AppError {
    AppError::new(
        "ANDROID_RUNTIME_NOT_MANAGED",
        format!("设备 {serial} 当前没有登记中的网络运行态。"),
    )
    .entity(serial)
}

fn stale_owner_error(serial: &str, current_epoch: Uuid) -> AppError {
    AppError::new(
        "ANDROID_RUNTIME_EPOCH_STALE",
        format!("设备 {serial} 的运行记录已变化。"),
    )
    .entity(serial)
    .epoch(current_epoch)
}

async fn authoritative_owner_conflict_error(executor: &SqliteExecutor, serial: &str) -> AppError {
    let target = serial.to_owned();
    match executor
        .execute(move |store| {
            store.load_android_runtime_owners().map(|owners| {
                owners
                    .into_iter()
                    .find(|record| record.owner.serial == target)
                    .map(|record| record.owner)
            })
        })
        .await
    {
        Ok(Some(owner)) => stale_owner_error(serial, owner.epoch),
        Ok(None) => not_managed_error(serial),
        Err(error) => owner_store_error(&error, serial, None),
    }
}

pub(super) fn runtime_mode(route_count: usize, uses_adb_reverse: bool) -> AndroidRuntimeOwnerMode {
    if route_count == 0 {
        AndroidRuntimeOwnerMode::DeviceOnly
    } else if uses_adb_reverse {
        AndroidRuntimeOwnerMode::AdbReverse
    } else {
        AndroidRuntimeOwnerMode::Lan
    }
}
