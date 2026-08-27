use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
};

use intercept_proxy_application::{
    AndroidNetworkActivation, AndroidRuntimeEndpointHealth, AndroidRuntimeEndpointViewModel,
    AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason,
    AppError, AppResult,
};
use serde_json::json;

use super::super::{
    ActiveReverseOwnership, ActiveRuntimeFacts, AndroidAdbAdapter, PreparedUsbProxyRuntime,
};
use super::{
    allocated_reverse_ports_avoiding, combine_operation_and_cleanup, reverse_create_error,
    runtime_mode,
};
use crate::adapters::android_adb::sha256_json;

struct ReverseCreationFailure {
    error: AppError,
    remaining_ports: Vec<u16>,
}

impl AndroidAdbAdapter {
    pub(in crate::adapters::android_adb) async fn build_lan_runtime_for_owner(
        &self,
        activation: &AndroidNetworkActivation,
        owner: &intercept_proxy_application::AndroidRuntimeOwnerViewModel,
        lan_host: std::net::Ipv4Addr,
    ) -> AppResult<(serde_json::Value, ActiveRuntimeFacts)> {
        let resolved_routes = resolve_routes(activation).await?;
        let listener_ports = BTreeMap::new();
        let (routes, endpoints) = build_routes_and_endpoints(
            resolved_routes,
            Some(lan_host),
            &listener_ports,
            &owner.serial,
            owner.epoch,
            intercept_proxy_application::AndroidRuntimeOwnerMode::Lan,
        );
        let route_fingerprint = sha256_json(&routes)?;
        let profile_fingerprint = sha256_json(&activation.profile)?;
        let route_count = activation.proxy_routes.len();
        let payload = json!({
            "routes": routes,
            "route_source": activation.proxy_routes,
            "profile_fingerprint": profile_fingerprint,
            "route_fingerprint": route_fingerprint,
            "route_count": route_count,
        });
        Ok((
            payload,
            ActiveRuntimeFacts {
                epoch: owner.epoch,
                serial: owner.serial.clone(),
                profile_id: activation.profile.id.clone(),
                profile_fingerprint,
                route_fingerprint,
                route_count,
                listener_ports,
                uses_adb_reverse: false,
                endpoints,
            },
        ))
    }

    pub(in crate::adapters::android_adb) async fn prepare_usb_proxy_runtime(
        &self,
        serial: &str,
        expected_epoch: Option<uuid::Uuid>,
        activation: &AndroidNetworkActivation,
        source: AndroidRuntimeOwnerSource,
    ) -> AppResult<PreparedUsbProxyRuntime> {
        if expected_epoch.is_none() {
            self.ensure_can_start(serial).await?;
        }
        let epoch = uuid::Uuid::new_v4();
        let previous = self.owner_state_snapshot_for(serial).await;
        validate_expected_epoch(serial, expected_epoch, &previous)?;
        let previous_owner = previous.runtime_owner;
        let previous_resume_state = previous.runtime_resume_state;
        let previous_reverse = previous.active_reverse;
        let previous_runtime = previous.active_runtime;
        let previous_endpoints = previous.runtime_endpoints;
        let reserved_ports = previous_reverse
            .as_ref()
            .map_or_else(Vec::new, |ownership| ownership.ports.clone());
        let resolved_routes = resolve_routes(activation).await?;
        let lan_host = self.preferred_lan_proxy_host(serial, activation).await;
        let uses_adb_reverse = lan_host.is_none();
        let listener_ports = if uses_adb_reverse {
            allocated_reverse_ports_avoiding(
                &activation.proxy_routes,
                reserved_ports.iter().copied(),
            )
        } else {
            BTreeMap::new()
        };
        let route_count = activation.proxy_routes.len();
        let mode = runtime_mode(route_count, uses_adb_reverse);
        let (routes, endpoints) = build_routes_and_endpoints(
            resolved_routes,
            lan_host,
            &listener_ports,
            serial,
            epoch,
            mode,
        );
        let route_fingerprint = sha256_json(&routes)?;
        let profile_fingerprint = sha256_json(&activation.profile)?;
        let payload = json!({
            "routes": routes,
            "route_source": activation.proxy_routes,
            "profile_fingerprint": profile_fingerprint,
            "route_fingerprint": route_fingerprint,
            "route_count": route_count,
        });
        let planned_ports = listener_ports.values().copied().collect::<Vec<_>>();
        let reverse = (!planned_ports.is_empty()).then(|| ActiveReverseOwnership {
            epoch,
            serial: serial.to_owned(),
            profile_id: activation.profile.id.clone(),
            ports: planned_ports.clone(),
        });
        let runtime = ActiveRuntimeFacts {
            epoch,
            serial: serial.to_owned(),
            profile_id: activation.profile.id.clone(),
            profile_fingerprint,
            route_fingerprint,
            route_count,
            listener_ports,
            uses_adb_reverse,
            endpoints,
        };
        let owner = runtime.owner(
            mode,
            source,
            AndroidRuntimeOwnerState::CleanupRequired,
            AndroidRuntimeOwnerTransitionReason::ReversePreparation,
        );
        let prepared = PreparedUsbProxyRuntime {
            payload,
            reverse,
            runtime,
            owner,
            previous_owner,
            previous_resume_state,
            previous_reverse,
            previous_runtime,
            previous_endpoints,
        };
        let cleanup_ports = prepared.all_cleanup_ports();
        self.stage_prepared_cleanup(&prepared, cleanup_ports, expected_epoch)
            .await?;
        if let Some(reverse) = prepared.reverse.as_ref()
            && let Err(failure) = self
                .create_reverse_mappings(activation, reverse, &prepared.runtime.listener_ports)
                .await
        {
            return Err(self.recover_failed_preparation(&prepared, failure).await);
        }
        Ok(prepared)
    }

    async fn create_reverse_mappings(
        &self,
        activation: &AndroidNetworkActivation,
        reverse: &ActiveReverseOwnership,
        listener_ports: &BTreeMap<String, u16>,
    ) -> Result<(), ReverseCreationFailure> {
        let mut created = Vec::new();
        for (listener_id, device_port) in listener_ports {
            let desktop_port = activation
                .proxy_routes
                .iter()
                .find(|route| route.listener_id == *listener_id)
                .map(|route| route.desktop_listener_port)
                .expect("allocated listener comes from activation route");
            if let Err(error) = self
                .run_for_serial(
                    &reverse.serial,
                    &[
                        "reverse",
                        &format!("tcp:{device_port}"),
                        &format!("tcp:{desktop_port}"),
                    ],
                    super::super::COMMAND_TIMEOUT,
                )
                .await
            {
                let primary = reverse_create_error(&error, *device_port, desktop_port);
                let cleanup = self.remove_reverse_ports(&reverse.serial, created).await;
                let error = cleanup.error.map_or(primary.clone(), |cleanup_error| {
                    combine_operation_and_cleanup(primary, &cleanup_error)
                });
                return Err(ReverseCreationFailure {
                    error,
                    remaining_ports: cleanup.remaining_ports,
                });
            }
            created.push(*device_port);
        }
        Ok(())
    }

    async fn recover_failed_preparation(
        &self,
        prepared: &PreparedUsbProxyRuntime,
        failure: ReverseCreationFailure,
    ) -> AppError {
        let recovery = if failure.remaining_ports.is_empty() {
            self.restore_previous_owner(prepared).await
        } else {
            let mut owner = prepared.owner.clone();
            let serial = owner.serial.clone();
            owner.state = AndroidRuntimeOwnerState::CleanupRequired;
            owner.transition_reason = AndroidRuntimeOwnerTransitionReason::ReverseCleanupRequired;
            let ports = prepared.cleanup_ports_with(failure.remaining_ports);
            match self.replace_owner_if_epoch(owner, ports).await {
                Ok(true) => Ok(()),
                Ok(false) => Err(self.runtime_owner_conflict_error(&serial).await),
                Err(error) => Err(error),
            }
        };
        match recovery {
            Ok(()) => failure.error,
            Err(error) => combine_operation_and_cleanup(failure.error, &error),
        }
    }
}

fn validate_expected_epoch(
    serial: &str,
    expected_epoch: Option<uuid::Uuid>,
    state: &super::super::AndroidOwnerState,
) -> AppResult<()> {
    let Some(expected_epoch) = expected_epoch else {
        return Ok(());
    };
    match state.runtime_owner.as_ref() {
        Some(owner) if owner.epoch == expected_epoch => Ok(()),
        Some(owner) => Err(AppError::new(
            "ANDROID_RUNTIME_EPOCH_STALE",
            format!("设备 {serial} 的运行记录已变化。"),
        )
        .entity(serial)
        .epoch(owner.epoch)),
        None => Err(AppError::new(
            "ANDROID_RUNTIME_NOT_MANAGED",
            format!("设备 {serial} 当前没有登记中的网络运行态。"),
        )
        .entity(serial)),
    }
}

impl PreparedUsbProxyRuntime {
    pub(in crate::adapters::android_adb) fn all_cleanup_ports(&self) -> Vec<u16> {
        self.cleanup_ports_with(
            self.reverse
                .as_ref()
                .map_or_else(Vec::new, |reverse| reverse.ports.clone()),
        )
    }

    pub(super) fn cleanup_ports_with(&self, extra: Vec<u16>) -> Vec<u16> {
        let mut ports = self
            .previous_reverse
            .as_ref()
            .map_or_else(Vec::new, |reverse| reverse.ports.clone());
        ports.extend(extra);
        ports.sort_unstable();
        ports.dedup();
        ports
    }

    pub(super) fn committed_ports_with(&self, remaining_old_ports: Vec<u16>) -> Vec<u16> {
        let mut ports = self
            .reverse
            .as_ref()
            .map_or_else(Vec::new, |reverse| reverse.ports.clone());
        ports.extend(remaining_old_ports);
        ports.sort_unstable();
        ports.dedup();
        ports
    }
}

async fn resolve_routes(
    activation: &AndroidNetworkActivation,
) -> AppResult<
    Vec<(
        &intercept_proxy_application::AndroidProxyRouteActivation,
        Vec<IpAddr>,
    )>,
> {
    let mut routes = Vec::with_capacity(activation.proxy_routes.len());
    for route in &activation.proxy_routes {
        routes.push((
            route,
            resolve_original_ips(route.original_destination.trim()).await?,
        ));
    }
    Ok(routes)
}

fn build_routes_and_endpoints(
    routes: Vec<(
        &intercept_proxy_application::AndroidProxyRouteActivation,
        Vec<IpAddr>,
    )>,
    lan_host: Option<std::net::Ipv4Addr>,
    listener_ports: &BTreeMap<String, u16>,
    serial: &str,
    epoch: uuid::Uuid,
    mode: intercept_proxy_application::AndroidRuntimeOwnerMode,
) -> (Vec<serde_json::Value>, Vec<AndroidRuntimeEndpointViewModel>) {
    let resolved_at = chrono::Utc::now();
    routes.into_iter().fold(
        (Vec::new(), Vec::new()),
        |(mut payload_routes, mut endpoints), (route, resolved_original_ips)| {
            let (proxy_host, proxy_port) = lan_host.map_or_else(
                || ("127.0.0.1".to_owned(), listener_ports[&route.listener_id]),
                |host| (host.to_string(), route.desktop_listener_port),
            );
            payload_routes.push(json!({
                "listener_id": route.listener_id,
                "original_destination": route.original_destination,
                "original_ports": route.original_ports,
                "resolved_original_ips": resolved_original_ips,
                "proxy_host": proxy_host,
                "proxy_port": proxy_port,
            }));
            endpoints.push(AndroidRuntimeEndpointViewModel {
                serial: serial.to_owned(),
                epoch,
                mode,
                original_destination: route.original_destination.clone(),
                original_ports: route.original_ports.clone(),
                resolved_original_ips: resolved_original_ips
                    .into_iter()
                    .map(|address| address.to_string())
                    .collect(),
                listener_id: route.listener_id.clone(),
                listener_name: route.listener_name.clone(),
                desktop_listener_port: route.desktop_listener_port,
                proxy_host,
                proxy_port,
                resolved_at,
                health: AndroidRuntimeEndpointHealth::Healthy,
            });
            (payload_routes, endpoints)
        },
    )
}

async fn resolve_original_ips(destination: &str) -> AppResult<Vec<IpAddr>> {
    if destination.parse::<IpAddr>().is_ok() || destination.contains('/') {
        return Ok(Vec::new());
    }
    let addresses = tokio::net::lookup_host((destination, 0))
        .await
        .map_err(|error| {
            AppError::new(
                "ANDROID_PROXY_DESTINATION_RESOLVE_FAILED",
                format!("透明代理原始域名 {destination} 无法解析：{error}"),
            )
        })?
        .map(|address| address.ip())
        .collect::<BTreeSet<_>>();
    if addresses.is_empty() {
        return Err(AppError::new(
            "ANDROID_PROXY_DESTINATION_RESOLVE_FAILED",
            format!("透明代理原始域名 {destination} 没有 A/AAAA 记录。"),
        ));
    }
    Ok(addresses.into_iter().collect())
}
