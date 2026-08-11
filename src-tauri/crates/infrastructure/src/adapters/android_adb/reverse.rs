//! 透明代理 ADB reverse 映射的准备、提交与回滚。
//!
//! `apply` 先解析域名并建立新映射，设备确认新方案 Running 后再替换活动所有权并清理
//! 旧映射。设备已接受请求但最终状态不确定时，同时保留新旧映射，避免晚到的设备启动
//! 使用一个已被桌面端撤销的端口。Android 数据面自身遵循 fail-open，不承诺无中断切换。

use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr},
};

use intercept_proxy_application::{AndroidNetworkActivation, AppError, AppResult};
use serde_json::json;

use super::{
    ActiveReverseOwnership, ActiveRuntimeFacts, AndroidAdbAdapter, COMMAND_TIMEOUT,
    PreparedUsbProxyRuntime, ReverseCleanupOutcome, sha256_json,
};
use crate::adapters::android_adb::command::is_missing_adb_listener_error;

mod lan;
mod support;

pub(super) use lan::lan_endpoint_is_eligible;
use lan::parse_device_lan_addresses;
#[cfg(test)]
pub(super) use support::allocated_reverse_ports;
use support::{
    allocated_reverse_ports_avoiding, reconcile_operation_cleanup, reverse_cleanup_error,
    reverse_create_error,
};
pub(super) use support::{
    combine_operation_and_cleanup, combine_stop_failures, reverse_mapping_present,
};

impl AndroidAdbAdapter {
    pub(super) async fn remove_reverse_ports(
        &self,
        serial: &str,
        ports: Vec<u16>,
    ) -> ReverseCleanupOutcome {
        let mut first_error = None;
        let mut remaining_ports = Vec::new();
        for port in ports {
            let result = self
                .run_for_serial(
                    serial,
                    &["reverse", "--remove", &format!("tcp:{port}")],
                    COMMAND_TIMEOUT,
                )
                .await;
            if let Err(error) = result
                && !is_missing_adb_listener_error(&error)
            {
                remaining_ports.push(port);
                if first_error.is_none() {
                    first_error = Some(reverse_cleanup_error(&error, port));
                }
            }
        }
        ReverseCleanupOutcome {
            remaining_ports,
            error: first_error,
        }
    }

    pub(super) async fn clear_active_reverse_ports(&self) -> AppResult<()> {
        // 清理成功前保留所有权；失败端口继续登记，后续 stop/紧急恢复可以重试。
        // 锁跨越 adb 调用，避免另一次 start 在清理期间覆盖所有权。
        let mut active = self.active_reverse.lock().await;
        let Some(ownership) = active.clone() else {
            *self.active_runtime.lock().await = None;
            return Ok(());
        };
        let outcome = self
            .remove_reverse_ports(&ownership.serial, ownership.ports)
            .await;
        if outcome.remaining_ports.is_empty() {
            *active = None;
            *self.active_runtime.lock().await = None;
        } else {
            *active = Some(ActiveReverseOwnership {
                ports: outcome.remaining_ports,
                ..ownership
            });
        }
        outcome.error.map_or(Ok(()), Err)
    }

    async fn create_reverse_mappings(
        &self,
        serial: &str,
        activation: &AndroidNetworkActivation,
        listener_ports: &BTreeMap<String, u16>,
    ) -> AppResult<Vec<u16>> {
        let mut created = Vec::new();
        for (listener_id, device_port) in listener_ports {
            let desktop_listener_port = activation
                .proxy_routes
                .iter()
                .find(|route| route.listener_id == *listener_id)
                .map(|route| route.desktop_listener_port)
                .expect("allocated listener comes from activation route");
            let result = self
                .run_for_serial(
                    serial,
                    &[
                        "reverse",
                        &format!("tcp:{device_port}"),
                        &format!("tcp:{desktop_listener_port}"),
                    ],
                    COMMAND_TIMEOUT,
                )
                .await;
            if let Err(error) = result {
                let error = reverse_create_error(&error, *device_port, desktop_listener_port);
                let cleanup = self.remove_reverse_ports(serial, created).await;
                if !cleanup.remaining_ports.is_empty() {
                    self.retain_reverse_ownership(ActiveReverseOwnership {
                        serial: serial.to_owned(),
                        profile_id: activation.profile.id.clone(),
                        ports: cleanup.remaining_ports,
                    })
                    .await;
                }
                return Err(cleanup.error.map_or(error.clone(), |cleanup_error| {
                    combine_operation_and_cleanup(error, &cleanup_error)
                }));
            }
            created.push(*device_port);
        }
        Ok(created)
    }

    async fn retain_reverse_ownership(&self, ownership: ActiveReverseOwnership) {
        if ownership.ports.is_empty() {
            return;
        }
        let mut active = self.active_reverse.lock().await;
        match active.as_mut() {
            Some(current) if current.serial == ownership.serial => {
                current.ports.extend(ownership.ports);
                current.ports.sort_unstable();
                current.ports.dedup();
            }
            None => *active = Some(ownership),
            Some(current) => debug_assert_eq!(current.serial, ownership.serial),
        }
    }

    pub(super) async fn prepare_usb_proxy_runtime(
        &self,
        activation: &AndroidNetworkActivation,
    ) -> AppResult<PreparedUsbProxyRuntime> {
        let serial = self.selected_serial()?;
        let profile_fingerprint = sha256_json(&activation.profile)?;
        let route_count = activation.proxy_routes.len();
        let reserved_ports = self
            .active_reverse
            .lock()
            .await
            .as_ref()
            .map_or_else(Vec::new, |ownership| ownership.ports.clone());

        // 先完成所有可能失败的 DNS 解析，再创建 reverse；失败不会影响旧 TUN。
        let mut resolved_routes = Vec::with_capacity(activation.proxy_routes.len());
        for route in &activation.proxy_routes {
            let destination = route.original_destination.trim();
            let resolved_original_ips =
                if destination.parse::<IpAddr>().is_ok() || destination.contains('/') {
                    Vec::new()
                } else {
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
                    addresses.into_iter().collect()
                };
            resolved_routes.push((route, resolved_original_ips));
        }

        let lan_host = self.preferred_lan_proxy_host(&serial, activation).await;
        let uses_adb_reverse = lan_host.is_none();
        let listener_ports = if uses_adb_reverse {
            allocated_reverse_ports_avoiding(
                &activation.proxy_routes,
                reserved_ports.iter().copied(),
            )
        } else {
            BTreeMap::new()
        };
        let created = if listener_ports.is_empty() {
            Vec::new()
        } else {
            self.create_reverse_mappings(&serial, activation, &listener_ports)
                .await?
        };

        let routes = resolved_routes
            .into_iter()
            .map(|(route, resolved_original_ips)| {
                let (proxy_host, proxy_port) = lan_host.map_or_else(
                    || ("127.0.0.1".to_owned(), listener_ports[&route.listener_id]),
                    |host| (host.to_string(), route.desktop_listener_port),
                );
                json!({
                    "listener_id": route.listener_id,
                    "original_destination": route.original_destination,
                    "original_ports": route.original_ports,
                    "resolved_original_ips": resolved_original_ips,
                    "proxy_host": proxy_host,
                    "proxy_port": proxy_port,
                })
            })
            .collect::<Vec<_>>();
        let route_fingerprint = sha256_json(&routes)?;
        let reverse = (!created.is_empty()).then(|| ActiveReverseOwnership {
            serial: serial.clone(),
            profile_id: activation.profile.id.clone(),
            ports: created,
        });
        Ok(PreparedUsbProxyRuntime {
            payload: json!({
                "routes": routes,
                "route_source": activation.proxy_routes,
                "profile_fingerprint": profile_fingerprint,
                "route_fingerprint": route_fingerprint,
                "route_count": route_count,
            }),
            reverse,
            runtime: ActiveRuntimeFacts {
                serial,
                profile_id: activation.profile.id.clone(),
                profile_fingerprint,
                route_fingerprint,
                route_count,
                listener_ports,
                uses_adb_reverse,
            },
        })
    }

    /// 同网段 LAN 可用时直接连接桌面 Listener。部分定制 Android 固件会让
    /// `adb reverse` 在设备端 connect 成功后立即关闭，却从未建立主机侧连接；
    /// 这种情况下应用只能看到 TLS EOF，LAN 路径可以绕过 OEM adbd 缺陷。
    async fn preferred_lan_proxy_host(
        &self,
        serial: &str,
        activation: &AndroidNetworkActivation,
    ) -> Option<Ipv4Addr> {
        if activation.proxy_routes.is_empty() {
            return None;
        }
        let output = self
            .run_for_serial(
                serial,
                &["shell", "ip", "-o", "-4", "addr", "show", "scope", "global"],
                COMMAND_TIMEOUT,
            )
            .await
            .ok()?;
        parse_device_lan_addresses(&output.stdout)
            .into_iter()
            .find_map(|(device_address, prefix)| {
                let host = self.lan_address.local_ipv4_for(device_address)?;
                lan_endpoint_is_eligible(host, device_address, prefix, &activation.proxy_routes)
                    .then_some(host)
            })
    }

    pub(super) async fn finish_prepared_network_update<T>(
        &self,
        prepared: PreparedUsbProxyRuntime,
        operation: AppResult<T>,
    ) -> AppResult<T> {
        match operation {
            Ok(value) => self
                .commit_prepared_network_update(prepared)
                .await
                .map(|()| value),
            Err(error) => {
                let cleanup = self.rollback_prepared_network_update(prepared).await;
                reconcile_operation_cleanup(Err(error), cleanup)
            }
        }
    }

    /// 设备已接受 start/apply，但桌面端未能确认最终状态。
    ///
    /// 此时不能回滚新 reverse：设备可能在超时之后才切换到新端口。保留新旧端口并
    /// 发布本次运行指纹，后续 status/stop/apply 可以核对或统一清理，不会把目标应用
    /// 留在指向已删除端口的断网状态。
    pub(super) async fn retain_uncertain_network_update<T>(
        &self,
        prepared: PreparedUsbProxyRuntime,
        error: AppError,
    ) -> AppResult<T> {
        if let Some(reverse) = prepared.reverse {
            self.retain_reverse_ownership(reverse).await;
        }
        *self.active_runtime.lock().await = Some(prepared.runtime);
        Err(error
            .retryable("设备最终状态尚未确认；已保留代理映射。请刷新运行状态，或执行停止后重试。"))
    }

    async fn rollback_prepared_network_update(
        &self,
        prepared: PreparedUsbProxyRuntime,
    ) -> AppResult<()> {
        let Some(reverse) = prepared.reverse else {
            return Ok(());
        };
        let outcome = self
            .remove_reverse_ports(&reverse.serial, reverse.ports)
            .await;
        if !outcome.remaining_ports.is_empty() {
            self.retain_reverse_ownership(ActiveReverseOwnership {
                ports: outcome.remaining_ports,
                ..reverse
            })
            .await;
        }
        outcome.error.map_or(Ok(()), Err)
    }

    async fn commit_prepared_network_update(
        &self,
        prepared: PreparedUsbProxyRuntime,
    ) -> AppResult<()> {
        let previous = {
            let mut active = self.active_reverse.lock().await;
            std::mem::replace(&mut *active, prepared.reverse)
        };
        *self.active_runtime.lock().await = Some(prepared.runtime);
        let Some(previous) = previous else {
            return Ok(());
        };
        let outcome = self
            .remove_reverse_ports(&previous.serial, previous.ports)
            .await;
        if !outcome.remaining_ports.is_empty() {
            self.retain_reverse_ownership(ActiveReverseOwnership {
                ports: outcome.remaining_ports,
                ..previous
            })
            .await;
        }
        outcome.error.map_or(Ok(()), Err)
    }
}
