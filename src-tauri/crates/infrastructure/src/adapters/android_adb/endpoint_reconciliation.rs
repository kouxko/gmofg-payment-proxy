use intercept_proxy_application::{
    AndroidNetworkActivation, AndroidRuntimeEndpointHealth, AndroidRuntimeEndpointViewModel,
    AndroidRuntimeOwnerMode, AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason,
    AndroidRuntimeOwnerViewModel, AppError, AppResult,
};
use serde_json::json;

use super::AndroidAdbAdapter;

pub(super) fn is_owner_unreachable(error: &AppError) -> bool {
    matches!(
        error.view_model.code.as_str(),
        "ANDROID_ADB_DEVICE_UNREACHABLE"
    )
}

impl AndroidAdbAdapter {
    pub(super) async fn reconcile_runtime_endpoints(
        &self,
        serial: String,
        activation: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
        let owner_for_gate = self.runtime_owner_snapshot_for(&serial).await;
        let _environment_apply_gates = self
            .acquire_environment_apply_gates(
                owner_for_gate
                    .as_ref()
                    .map(|owner| owner.profile_id.as_str())
                    .or_else(|| activation.as_ref().map(|value| value.profile.id.as_str())),
                Some(&serial),
            )
            .await;
        let gate = self.device_operations.gate(&serial);
        let _operation = gate.lock().await;
        let Some(owner) = self.runtime_owner_snapshot_for(&serial).await else {
            return Ok(Vec::new());
        };
        let owner_state = self.owner_state_snapshot_for(&serial).await;
        let persisted = owner_state.runtime_endpoints.clone();
        if owner.mode != AndroidRuntimeOwnerMode::Lan {
            return Ok(endpoints_with_owner_health(persisted, owner.state));
        }
        let Some(activation) = activation.filter(|activation| {
            activation.profile.id == owner.profile_id
                && persisted.iter().all(|endpoint| {
                    endpoint.serial == owner.serial && endpoint.epoch == owner.epoch
                })
        }) else {
            return Ok(endpoints_with_health(
                persisted,
                AndroidRuntimeEndpointHealth::Faulted,
            ));
        };
        let lan_host = match self
            .preferred_lan_proxy_host_strict(&owner.serial, &activation)
            .await
        {
            Ok(Some(host)) => host,
            Err(error) if is_owner_unreachable(&error) => {
                self.mark_owner_waiting_reconnect(&serial, owner.epoch)
                    .await?;
                return Ok(endpoints_with_health(
                    persisted,
                    AndroidRuntimeEndpointHealth::WaitingReconnect,
                ));
            }
            Ok(None) | Err(_) => return self.fault_lan_endpoints(owner, persisted).await,
        };
        let host = lan_host.to_string();
        let has_active_runtime = owner_state
            .active_runtime
            .as_ref()
            .is_some_and(|runtime| runtime.serial == owner.serial && runtime.epoch == owner.epoch);
        if owner.state == AndroidRuntimeOwnerState::Active
            && has_active_runtime
            && !persisted.is_empty()
            && persisted.iter().all(|endpoint| endpoint.proxy_host == host)
        {
            return Ok(endpoints_with_owner_health(persisted, owner.state));
        }

        let Ok((proxy_runtime, runtime)) = self
            .build_lan_runtime_for_owner(&activation, &owner, lan_host)
            .await
        else {
            return self.fault_lan_endpoints(owner, persisted).await;
        };
        let payload = json!({"profile": activation.profile, "proxy_runtime": proxy_runtime});
        let refreshed = match self.protocol_request(&owner.serial, "apply", payload).await {
            Ok(status) => self.confirm_network_running(&runtime, status).await,
            Err(error) => Err(error),
        };
        if refreshed.is_err() {
            return self.fault_lan_endpoints(owner, runtime.endpoints).await;
        }
        let mut active_owner = owner;
        active_owner.state = AndroidRuntimeOwnerState::Active;
        active_owner.transition_reason = AndroidRuntimeOwnerTransitionReason::LanEndpointReapplied;
        active_owner.updated_at = chrono::Utc::now();
        let serial = active_owner.serial.clone();
        if !self
            .replace_owner_endpoints_and_runtime_if_epoch(
                active_owner,
                runtime.endpoints.clone(),
                runtime.clone(),
            )
            .await?
        {
            return Err(self.runtime_owner_conflict_error(&serial).await);
        }
        Ok(runtime.endpoints)
    }

    async fn fault_lan_endpoints(
        &self,
        mut owner: AndroidRuntimeOwnerViewModel,
        endpoints: Vec<AndroidRuntimeEndpointViewModel>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
        let endpoints = endpoints_with_health(endpoints, AndroidRuntimeEndpointHealth::Faulted);
        owner.state = AndroidRuntimeOwnerState::Faulted;
        owner.transition_reason = AndroidRuntimeOwnerTransitionReason::LanEndpointFaulted;
        owner.updated_at = chrono::Utc::now();
        let serial = owner.serial.clone();
        if !self
            .replace_owner_endpoints_if_epoch(owner, endpoints.clone())
            .await?
        {
            return Err(self.runtime_owner_conflict_error(&serial).await);
        }
        Ok(endpoints)
    }
}

fn endpoints_with_owner_health(
    endpoints: Vec<AndroidRuntimeEndpointViewModel>,
    state: AndroidRuntimeOwnerState,
) -> Vec<AndroidRuntimeEndpointViewModel> {
    let health = match state {
        AndroidRuntimeOwnerState::Active => AndroidRuntimeEndpointHealth::Healthy,
        AndroidRuntimeOwnerState::WaitingReconnect => {
            AndroidRuntimeEndpointHealth::WaitingReconnect
        }
        AndroidRuntimeOwnerState::Uncertain
        | AndroidRuntimeOwnerState::CleanupRequired
        | AndroidRuntimeOwnerState::StopFailed
        | AndroidRuntimeOwnerState::Faulted => AndroidRuntimeEndpointHealth::Faulted,
    };
    endpoints_with_health(endpoints, health)
}

fn endpoints_with_health(
    mut endpoints: Vec<AndroidRuntimeEndpointViewModel>,
    health: AndroidRuntimeEndpointHealth,
) -> Vec<AndroidRuntimeEndpointViewModel> {
    for endpoint in &mut endpoints {
        endpoint.health = health;
    }
    endpoints
}
