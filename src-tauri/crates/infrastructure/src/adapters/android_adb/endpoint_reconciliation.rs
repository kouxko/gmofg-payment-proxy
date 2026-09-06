use super::{ActiveRuntimeFacts, AndroidAdbAdapter, lease::control_request_payload, sha256_json};
use intercept_proxy_application::{
    AndroidNetworkActivation, AndroidRuntimeEndpointHealth, AndroidRuntimeEndpointViewModel,
    AndroidRuntimeOwnerMode, AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason,
    AndroidRuntimeOwnerViewModel, AppError, AppResult,
};
use serde_json::json;

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
        let active_runtime = owner_state
            .active_runtime
            .as_ref()
            .filter(|runtime| {
                runtime.serial == owner.serial
                    && runtime.epoch == owner.epoch
                    && runtime.profile_id == owner.profile_id
            })
            .cloned();
        if owner.state != AndroidRuntimeOwnerState::Active || active_runtime.is_none() {
            let state = if owner.state == AndroidRuntimeOwnerState::Active {
                AndroidRuntimeOwnerState::Faulted
            } else {
                owner.state
            };
            return Ok(endpoints_with_owner_health(persisted, state));
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
        let active_runtime = active_runtime.expect("active runtime was checked above");
        if sha256_json(&activation.profile)? != active_runtime.profile_fingerprint {
            return Ok(endpoints_with_health(
                persisted,
                AndroidRuntimeEndpointHealth::Faulted,
            ));
        }
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
        if !persisted.is_empty() && persisted.iter().all(|endpoint| endpoint.proxy_host == host) {
            return Ok(endpoints_with_owner_health(persisted, owner.state));
        }

        let Ok((proxy_runtime, runtime)) =
            refresh_lan_runtime_host(&activation, active_runtime, lan_host)
        else {
            return self.fault_lan_endpoints(owner, persisted).await;
        };
        let payload = control_request_payload(&activation.profile, &proxy_runtime, owner.epoch);
        let operation = if activation.profile.stop_vpn_on_control_loss {
            "apply_active"
        } else {
            "apply"
        };
        let refreshed = match self
            .protocol_request(&owner.serial, operation, payload)
            .await
        {
            Ok(status) => self.confirm_network_running(&runtime, status).await,
            Err(error) => Err(error),
        };
        if refreshed.is_err() {
            return self.fault_lan_endpoints(owner, runtime.endpoints).await;
        }
        self.commit_lan_runtime(owner, runtime).await
    }

    async fn commit_lan_runtime(
        &self,
        owner: AndroidRuntimeOwnerViewModel,
        runtime: ActiveRuntimeFacts,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
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

fn refresh_lan_runtime_host(
    activation: &AndroidNetworkActivation,
    mut runtime: ActiveRuntimeFacts,
    lan_host: std::net::Ipv4Addr,
) -> AppResult<(serde_json::Value, ActiveRuntimeFacts)> {
    let host = lan_host.to_string();
    for endpoint in &mut runtime.endpoints {
        endpoint.proxy_host.clone_from(&host);
    }
    let routes = runtime
        .endpoints
        .iter()
        .map(|endpoint| {
            json!({
                "listener_id": endpoint.listener_id,
                "original_destination": endpoint.original_destination,
                "original_ports": endpoint.original_ports,
                "resolved_original_ips": endpoint.resolved_original_ips,
                "proxy_host": endpoint.proxy_host,
                "proxy_port": endpoint.proxy_port,
            })
        })
        .collect::<Vec<_>>();
    runtime.route_fingerprint = sha256_json(&routes)?;
    let payload = json!({
        "routes": routes,
        "route_source": activation.proxy_routes,
        "profile_fingerprint": runtime.profile_fingerprint,
        "route_fingerprint": runtime.route_fingerprint,
        "route_count": runtime.route_count,
    });
    Ok((payload, runtime))
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
