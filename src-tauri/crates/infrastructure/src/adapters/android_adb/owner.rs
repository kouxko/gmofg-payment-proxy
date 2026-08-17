use chrono::Utc;
use intercept_proxy_application::{
    AndroidNetworkState, AndroidRuntimeOwnerMode, AndroidRuntimeOwnerState,
    AndroidRuntimeOwnerTransitionReason, AndroidRuntimeOwnerViewModel, AppError, AppResult,
};
use uuid::Uuid;

use super::{AndroidAdbAdapter, PreparedUsbProxyRuntime};
use crate::AndroidRuntimeOwnerRecord;

impl AndroidAdbAdapter {
    pub(super) async fn runtime_owner_snapshot(&self) -> Option<AndroidRuntimeOwnerViewModel> {
        self.runtime_owner.lock().await.clone()
    }

    pub(super) async fn required_runtime_owner(&self) -> AppResult<AndroidRuntimeOwnerViewModel> {
        self.runtime_owner_snapshot().await.ok_or_else(|| {
            AppError::new(
                "ANDROID_RUNTIME_OWNER_NOT_FOUND",
                "当前没有登记中的 Android 网络运行设备。",
            )
        })
    }

    pub(super) async fn ensure_selected_can_activate(&self, serial: &str) -> AppResult<()> {
        if let Some(owner) = self.runtime_owner_snapshot().await
            && owner.serial != serial
        {
            return Err(AppError::new(
                "ANDROID_RUNTIME_OWNED_BY_ANOTHER_DEVICE",
                format!(
                    "设备 {} 仍拥有网络运行态；不能由所选设备 {serial} 接管。",
                    owner.serial
                ),
            )
            .retryable("请先停止或恢复当前运行设备，再启动所选设备。"));
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
        self.save_owner(owner).await
    }

    pub(super) async fn stage_prepared_cleanup(
        &self,
        prepared: &PreparedUsbProxyRuntime,
        cleanup_ports: Vec<u16>,
    ) -> AppResult<()> {
        let mut owner = prepared.owner.clone();
        owner.state = AndroidRuntimeOwnerState::CleanupRequired;
        owner.transition_reason = AndroidRuntimeOwnerTransitionReason::ReversePreparation;
        owner.updated_at = Utc::now();
        let record = AndroidRuntimeOwnerRecord {
            owner: owner.clone(),
            reverse_ports: cleanup_ports.clone(),
            resume_state: None,
        };
        self.runtime_store
            .save_android_runtime_owner(&record)
            .map_err(|error| owner_store_error(&error))?;
        self.publish_record_in_memory(owner, cleanup_ports, None)
            .await;
        Ok(())
    }

    pub(super) async fn save_owner(&self, owner: AndroidRuntimeOwnerViewModel) -> AppResult<()> {
        // 先发布进程内所有权；即使磁盘暂时失败，当前进程仍必须能准确 stop owner。
        *self.runtime_owner.lock().await = Some(owner.clone());
        let reverse_ports = self
            .active_reverse
            .lock()
            .await
            .as_ref()
            .filter(|reverse| reverse.serial == owner.serial && reverse.epoch == owner.epoch)
            .map_or_else(Vec::new, |reverse| reverse.ports.clone());
        self.runtime_store
            .save_android_runtime_owner(&AndroidRuntimeOwnerRecord {
                owner,
                reverse_ports,
                resume_state: *self.runtime_resume_state.lock().await,
            })
            .map_err(|error| owner_store_error(&error))?;
        Ok(())
    }

    pub(super) async fn replace_owner_if_epoch(
        &self,
        owner: AndroidRuntimeOwnerViewModel,
        reverse_ports: Vec<u16>,
    ) -> AppResult<bool> {
        let resume_state = *self.runtime_resume_state.lock().await;
        self.replace_owner_with_resume(owner, reverse_ports, resume_state)
            .await
    }

    async fn replace_owner_with_resume(
        &self,
        owner: AndroidRuntimeOwnerViewModel,
        reverse_ports: Vec<u16>,
        resume_state: Option<AndroidRuntimeOwnerState>,
    ) -> AppResult<bool> {
        let expected_epoch = owner.epoch;
        let record = AndroidRuntimeOwnerRecord {
            owner: owner.clone(),
            reverse_ports: reverse_ports.clone(),
            resume_state,
        };
        let replaced = self
            .runtime_store
            .replace_android_runtime_owner_if_epoch(expected_epoch, &record)
            .map_err(|error| owner_store_error(&error))?;
        if replaced {
            self.publish_record_in_memory(owner, reverse_ports, resume_state)
                .await;
        }
        Ok(replaced)
    }

    pub(super) async fn restore_previous_owner(
        &self,
        prepared: &PreparedUsbProxyRuntime,
    ) -> AppResult<()> {
        if let Some(owner) = prepared.previous_owner.clone() {
            let ports = prepared
                .previous_reverse
                .as_ref()
                .map_or_else(Vec::new, |reverse| reverse.ports.clone());
            self.runtime_store
                .save_android_runtime_owner(&AndroidRuntimeOwnerRecord {
                    owner: owner.clone(),
                    reverse_ports: ports.clone(),
                    resume_state: prepared.previous_resume_state,
                })
                .map_err(|error| owner_store_error(&error))?;
            self.publish_record_in_memory(owner, ports, prepared.previous_resume_state)
                .await;
        } else if self.clear_owner_if_epoch(prepared.owner.epoch).await? {
            *self.active_reverse.lock().await = None;
        }
        *self.active_runtime.lock().await = prepared.previous_runtime.clone();
        Ok(())
    }

    pub(super) async fn mark_owner_waiting_reconnect(&self, expected_epoch: Uuid) -> AppResult<()> {
        let owner = self.runtime_owner_snapshot().await;
        let Some(mut owner) = owner.filter(|owner| owner.epoch == expected_epoch) else {
            return Ok(());
        };
        let resume_state = if owner.state == AndroidRuntimeOwnerState::WaitingReconnect {
            *self.runtime_resume_state.lock().await
        } else {
            Some(owner.state)
        };
        owner.state = AndroidRuntimeOwnerState::WaitingReconnect;
        owner.transition_reason = AndroidRuntimeOwnerTransitionReason::DeviceDisconnected;
        owner.updated_at = Utc::now();
        let ports = self.current_reverse_ports(expected_epoch).await;
        self.replace_owner_with_resume(owner, ports, resume_state)
            .await
            .map(|_| ())
    }

    pub(super) async fn mark_owner_reconnected(
        &self,
        expected_epoch: Uuid,
        observed: Option<AndroidNetworkState>,
    ) -> AppResult<()> {
        let owner = self.runtime_owner_snapshot().await;
        if !owner.as_ref().is_some_and(|owner| {
            owner.epoch == expected_epoch
                && owner.state == AndroidRuntimeOwnerState::WaitingReconnect
        }) {
            return Ok(());
        }
        let resume_state = *self.runtime_resume_state.lock().await;
        let state = match resume_state {
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
        let mut owner = owner.unwrap();
        owner.state = state;
        owner.transition_reason = AndroidRuntimeOwnerTransitionReason::DeviceReconnected;
        owner.updated_at = Utc::now();
        let ports = self.current_reverse_ports(expected_epoch).await;
        self.replace_owner_with_resume(owner, ports, None)
            .await
            .map(|_| ())
    }

    pub(super) async fn mark_owner_stop_failed(
        &self,
        expected_epoch: Uuid,
        _reason: String,
    ) -> AppResult<()> {
        let owner = self.runtime_owner_snapshot().await;
        let Some(mut owner) = owner.filter(|owner| owner.epoch == expected_epoch) else {
            return Ok(());
        };
        owner.state = AndroidRuntimeOwnerState::StopFailed;
        owner.transition_reason = AndroidRuntimeOwnerTransitionReason::StopFailed;
        owner.updated_at = Utc::now();
        let ports = self.current_reverse_ports(expected_epoch).await;
        self.replace_owner_with_resume(owner, ports, None)
            .await
            .map(|_| ())
    }

    async fn current_reverse_ports(&self, expected_epoch: Uuid) -> Vec<u16> {
        self.active_reverse
            .lock()
            .await
            .as_ref()
            .filter(|reverse| reverse.epoch == expected_epoch)
            .map_or_else(Vec::new, |reverse| reverse.ports.clone())
    }

    async fn publish_record_in_memory(
        &self,
        owner: AndroidRuntimeOwnerViewModel,
        reverse_ports: Vec<u16>,
        resume_state: Option<AndroidRuntimeOwnerState>,
    ) {
        let reverse = (!reverse_ports.is_empty()).then(|| super::ActiveReverseOwnership {
            epoch: owner.epoch,
            serial: owner.serial.clone(),
            profile_id: owner.profile_id.clone(),
            ports: reverse_ports,
        });
        *self.runtime_owner.lock().await = Some(owner);
        *self.runtime_resume_state.lock().await = resume_state;
        *self.active_reverse.lock().await = reverse;
    }

    pub(super) async fn clear_owner_if_epoch(&self, expected_epoch: Uuid) -> AppResult<bool> {
        let cleared = self
            .runtime_store
            .clear_android_runtime_owner(expected_epoch)
            .map_err(|error| owner_store_error(&error))?;
        if cleared {
            let mut owner = self.runtime_owner.lock().await;
            if owner
                .as_ref()
                .is_some_and(|owner| owner.epoch == expected_epoch)
            {
                *owner = None;
                *self.runtime_resume_state.lock().await = None;
            }
            let mut runtime = self.active_runtime.lock().await;
            if runtime
                .as_ref()
                .is_some_and(|runtime| runtime.epoch == expected_epoch)
            {
                *runtime = None;
            }
            let mut reverse = self.active_reverse.lock().await;
            if reverse
                .as_ref()
                .is_some_and(|reverse| reverse.epoch == expected_epoch)
            {
                *reverse = None;
            }
        }
        Ok(cleared)
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

fn owner_store_error(error: &crate::InfrastructureError) -> AppError {
    AppError::new(
        "ANDROID_RUNTIME_OWNER_PERSISTENCE_FAILED",
        format!("Android 运行设备记录读写失败：{error}"),
    )
}
