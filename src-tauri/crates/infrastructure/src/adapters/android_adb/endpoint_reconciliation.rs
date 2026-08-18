use intercept_proxy_application::{
    AndroidNetworkActivation, AndroidRuntimeEndpointHealth, AndroidRuntimeEndpointViewModel,
    AndroidRuntimeOwnerMode, AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason,
    AndroidRuntimeOwnerViewModel, AppError, AppResult,
};
use serde_json::json;

use super::{AndroidAdbAdapter, is_owner_unreachable};

impl AndroidAdbAdapter {
    pub(super) async fn reconcile_runtime_endpoints(
        &self,
        activation: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
        let _operation = self.network_operation.lock().await;
        let Some(owner) = self.runtime_owner_snapshot().await else {
            return Ok(Vec::new());
        };
        let persisted = self.runtime_endpoints.lock().await.clone();
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
            Ok(None) => return self.fault_lan_endpoints(owner, persisted).await,
            Err(error) if is_owner_unreachable(&error) => {
                self.mark_owner_waiting_reconnect(owner.epoch).await?;
                return Ok(endpoints_with_health(
                    persisted,
                    AndroidRuntimeEndpointHealth::WaitingReconnect,
                ));
            }
            Err(_) => return self.fault_lan_endpoints(owner, persisted).await,
        };
        let host = lan_host.to_string();
        let has_active_runtime = self
            .active_runtime
            .lock()
            .await
            .as_ref()
            .is_some_and(|runtime| runtime.serial == owner.serial && runtime.epoch == owner.epoch);
        if owner.state == AndroidRuntimeOwnerState::Active
            && has_active_runtime
            && !persisted.is_empty()
            && persisted.iter().all(|endpoint| endpoint.proxy_host == host)
        {
            return Ok(endpoints_with_owner_health(persisted, owner.state));
        }

        let (proxy_runtime, runtime) = match self
            .build_lan_runtime_for_owner(&activation, &owner, lan_host)
            .await
        {
            Ok(runtime) => runtime,
            Err(_) => return self.fault_lan_endpoints(owner, persisted).await,
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
        if !self
            .replace_owner_endpoints_if_epoch(active_owner, runtime.endpoints.clone())
            .await?
        {
            return Err(AppError::new(
                "ANDROID_RUNTIME_OWNER_STALE_EPOCH",
                "Android 运行设备记录已变化，本次 LAN 端点刷新未覆盖新记录。",
            ));
        }
        *self.active_runtime.lock().await = Some(runtime.clone());
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
        if !self
            .replace_owner_endpoints_if_epoch(owner, endpoints.clone())
            .await?
        {
            return Err(AppError::new(
                "ANDROID_RUNTIME_OWNER_STALE_EPOCH",
                "Android 运行设备记录已变化，本次 LAN 故障状态未覆盖新记录。",
            ));
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
