use std::{collections::BTreeMap, future::Future, sync::Arc, time::Duration};

use intercept_proxy_application::{AndroidNetworkProfile, AndroidRuntimeOwnerState, AppResult};
use serde_json::json;
use tokio::time::timeout;

use super::AndroidAdbAdapter;
use super::AndroidOwnerState;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
const HEARTBEAT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONCURRENT_HEARTBEATS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ControlLeaseHeartbeatFailure {
    serial: String,
    epoch: uuid::Uuid,
    code: String,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ControlLeaseHeartbeatOutcome {
    failure: Option<ControlLeaseHeartbeatFailure>,
}

pub(super) fn control_lease_payload(
    profile: &AndroidNetworkProfile,
    epoch: uuid::Uuid,
) -> Option<serde_json::Value> {
    profile
        .stop_vpn_on_control_loss
        .then(|| json!({"owner_epoch": epoch.to_string(), "timeout_millis": 5_000}))
}

pub(super) fn control_request_payload(
    profile: &AndroidNetworkProfile,
    proxy_runtime: &serde_json::Value,
    epoch: uuid::Uuid,
) -> serde_json::Value {
    let control_lease = control_lease_payload(profile, epoch);
    json!({"profile": profile, "proxy_runtime": proxy_runtime, "control_lease": control_lease})
}

impl AndroidAdbAdapter {
    pub fn start_control_lease_heartbeat(adapter: &Arc<Self>) {
        let adapter = Arc::downgrade(adapter);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                interval.tick().await;
                let Some(adapter) = adapter.upgrade() else {
                    return;
                };
                adapter.renew_control_leases_once().await;
            }
        });
    }

    async fn renew_control_leases_once(self: &Arc<Self>) {
        let targets = {
            let states = self.owner_states.lock().await;
            lease_heartbeat_targets(&states)
        };
        let adapter = Arc::clone(self);
        let outcomes =
            renew_targets_concurrently(targets, HEARTBEAT_ATTEMPT_TIMEOUT, move |serial, epoch| {
                let adapter = Arc::clone(&adapter);
                async move { adapter.renew_control_lease_target(&serial, epoch).await }
            })
            .await;
        for failure in outcomes.into_iter().filter_map(|outcome| outcome.failure) {
            tracing::warn!(
                android_serial = %failure.serial,
                runtime_epoch = %failure.epoch,
                error_code = %failure.code,
                error_message = %failure.message,
                "Android control lease heartbeat failed"
            );
        }
    }

    async fn renew_control_lease_target(&self, serial: &str, epoch: uuid::Uuid) -> AppResult<()> {
        let gate = self.device_operations.gate(serial);
        let _operation = gate.lock().await;
        let current = self.owner_state_snapshot_for(serial).await;
        let is_current = current.runtime_owner.as_ref().is_some_and(|owner| {
            owner.epoch == epoch
                && matches!(
                    owner.state,
                    AndroidRuntimeOwnerState::Active | AndroidRuntimeOwnerState::Uncertain
                )
        }) && current
            .active_runtime
            .as_ref()
            .is_some_and(|runtime| runtime.epoch == epoch && runtime.stop_vpn_on_control_loss);
        if !is_current {
            return Ok(());
        }
        self.protocol_request(
            serial,
            "heartbeat",
            json!({"owner_epoch": epoch.to_string()}),
        )
        .await?;
        Ok(())
    }
}

async fn renew_targets_concurrently<F, Fut>(
    targets: Vec<(String, uuid::Uuid)>,
    attempt_timeout: Duration,
    renew: F,
) -> Vec<ControlLeaseHeartbeatOutcome>
where
    F: Fn(String, uuid::Uuid) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = AppResult<()>> + Send + 'static,
{
    let renew = Arc::new(renew);
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_HEARTBEATS));
    let tasks = targets
        .into_iter()
        .map(|(serial, epoch)| {
            let renew = Arc::clone(&renew);
            let permits = Arc::clone(&permits);
            let task_serial = serial.clone();
            let task = tokio::spawn(async move {
                let _permit = permits
                    .acquire_owned()
                    .await
                    .expect("heartbeat semaphore remains open");
                timeout(attempt_timeout, renew(task_serial, epoch)).await
            });
            (serial, epoch, task)
        })
        .collect::<Vec<_>>();
    let mut outcomes = Vec::with_capacity(tasks.len());
    for (serial, epoch, task) in tasks {
        let failure = match task.await {
            Ok(Ok(Ok(()))) => None,
            Ok(Ok(Err(error))) => Some(ControlLeaseHeartbeatFailure {
                serial,
                epoch,
                code: error.view_model.code,
                message: error.view_model.message,
            }),
            Ok(Err(_)) => Some(ControlLeaseHeartbeatFailure {
                serial,
                epoch,
                code: "ANDROID_CONTROL_LEASE_HEARTBEAT_TIMEOUT".into(),
                message: "Android 控制租约续期未在 2 秒内完成。".into(),
            }),
            Err(error) => Some(ControlLeaseHeartbeatFailure {
                serial,
                epoch,
                code: "ANDROID_CONTROL_LEASE_HEARTBEAT_TASK_FAILED".into(),
                message: format!("Android 控制租约续期任务异常结束：{error}"),
            }),
        };
        outcomes.push(ControlLeaseHeartbeatOutcome { failure });
    }
    outcomes
}

fn lease_heartbeat_targets(
    states: &BTreeMap<String, AndroidOwnerState>,
) -> Vec<(String, uuid::Uuid)> {
    states
        .values()
        .filter_map(|state| {
            let owner = state.runtime_owner.as_ref()?;
            let runtime = state.active_runtime.as_ref()?;
            (runtime.stop_vpn_on_control_loss
                && runtime.epoch == owner.epoch
                && matches!(
                    owner.state,
                    AndroidRuntimeOwnerState::Active | AndroidRuntimeOwnerState::Uncertain
                ))
            .then(|| (owner.serial.clone(), owner.epoch))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::android_adb::ActiveRuntimeFacts;
    use intercept_proxy_application::{
        AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource, AndroidRuntimeOwnerTransitionReason,
        AndroidRuntimeOwnerViewModel, AppResult,
    };

    fn runtime_owner(
        serial: &str,
        state: AndroidRuntimeOwnerState,
    ) -> AndroidRuntimeOwnerViewModel {
        AndroidRuntimeOwnerViewModel {
            serial: serial.into(),
            epoch: uuid::Uuid::new_v4(),
            mode: AndroidRuntimeOwnerMode::AdbReverse,
            profile_id: "profile-test".into(),
            state,
            source: AndroidRuntimeOwnerSource::Start,
            transition_reason: AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
            updated_at: chrono::Utc::now(),
        }
    }

    fn activation_runtime() -> ActiveRuntimeFacts {
        ActiveRuntimeFacts {
            epoch: uuid::Uuid::new_v4(),
            serial: "device".into(),
            profile_id: "profile-test".into(),
            profile_fingerprint: "profile-fingerprint".into(),
            route_fingerprint: "route-fingerprint".into(),
            route_count: 1,
            stop_vpn_on_control_loss: true,
            listener_ports: BTreeMap::new(),
            uses_adb_reverse: true,
            endpoints: Vec::new(),
        }
    }

    #[test]
    fn targets_are_per_serial_and_exclude_disabled_or_disconnected_owners() {
        let mut active_runtime = activation_runtime();
        active_runtime.serial = "device-a".into();
        let active_owner = runtime_owner("device-a", AndroidRuntimeOwnerState::Active);
        active_runtime.epoch = active_owner.epoch;

        let mut disabled_runtime = activation_runtime();
        disabled_runtime.serial = "device-b".into();
        disabled_runtime.stop_vpn_on_control_loss = false;
        let disabled_owner = runtime_owner("device-b", AndroidRuntimeOwnerState::Active);
        disabled_runtime.epoch = disabled_owner.epoch;

        let mut disconnected_runtime = activation_runtime();
        disconnected_runtime.serial = "device-c".into();
        let disconnected_owner =
            runtime_owner("device-c", AndroidRuntimeOwnerState::WaitingReconnect);
        disconnected_runtime.epoch = disconnected_owner.epoch;

        let states = BTreeMap::from([
            (
                "device-a".into(),
                AndroidOwnerState {
                    runtime_owner: Some(active_owner.clone()),
                    active_runtime: Some(active_runtime),
                    ..AndroidOwnerState::default()
                },
            ),
            (
                "device-b".into(),
                AndroidOwnerState {
                    runtime_owner: Some(disabled_owner),
                    active_runtime: Some(disabled_runtime),
                    ..AndroidOwnerState::default()
                },
            ),
            (
                "device-c".into(),
                AndroidOwnerState {
                    runtime_owner: Some(disconnected_owner),
                    active_runtime: Some(disconnected_runtime),
                    ..AndroidOwnerState::default()
                },
            ),
        ]);

        assert_eq!(
            lease_heartbeat_targets(&states),
            vec![("device-a".into(), active_owner.epoch)]
        );
    }

    #[test]
    fn request_payload_uses_fixed_five_second_epoch_lease_only_when_enabled() {
        let epoch = uuid::Uuid::new_v4();
        let mut profile = AndroidNetworkProfile {
            id: "profile".into(),
            name: "Profile".into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            proxy_routes: Vec::new(),
            confirmed_shared_uids: std::collections::BTreeSet::new(),
            auto_resume_after_reboot: false,
            stop_vpn_on_control_loss: true,
            weak_network: intercept_proxy_application::WeakNetworkProfile::default(),
        };

        let enabled = control_request_payload(&profile, &json!({"routes": []}), epoch);
        assert_eq!(enabled["control_lease"]["owner_epoch"], epoch.to_string());
        assert_eq!(enabled["control_lease"]["timeout_millis"], 5_000);

        profile.stop_vpn_on_control_loss = false;
        let disabled = control_request_payload(&profile, &json!({"routes": []}), epoch);
        assert!(disabled["control_lease"].is_null());
    }

    #[tokio::test]
    async fn blocked_device_does_not_starve_another_serial_and_failures_keep_epoch_context() {
        let epoch_a = uuid::Uuid::new_v4();
        let epoch_b = uuid::Uuid::new_v4();
        let renewed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = Arc::clone(&renewed);

        let outcomes = renew_targets_concurrently(
            vec![("device-a".into(), epoch_a), ("device-b".into(), epoch_b)],
            Duration::from_millis(25),
            move |serial, epoch| {
                let observed = Arc::clone(&observed);
                async move {
                    if serial == "device-a" {
                        std::future::pending::<AppResult<()>>().await
                    } else {
                        observed.lock().unwrap().push((serial, epoch));
                        Ok(())
                    }
                }
            },
        )
        .await;

        assert_eq!(*renewed.lock().unwrap(), vec![("device-b".into(), epoch_b)]);
        assert_eq!(outcomes.len(), 2);
        let failure = outcomes
            .iter()
            .find_map(|outcome| outcome.failure.as_ref())
            .expect("device A timeout must be observable");
        assert_eq!(failure.serial, "device-a");
        assert_eq!(failure.epoch, epoch_a);
        assert_eq!(failure.code, "ANDROID_CONTROL_LEASE_HEARTBEAT_TIMEOUT");
    }
}
