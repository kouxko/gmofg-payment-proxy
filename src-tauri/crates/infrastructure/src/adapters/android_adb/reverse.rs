//! 透明代理 ADB reverse 映射的准备、提交与回滚。
//!
//! `apply` 先解析域名并建立新映射，设备确认新方案 Running 后再替换活动所有权并清理
//! 旧映射。设备已接受请求但最终状态不确定时，同时保留新旧映射，避免晚到的设备启动
//! 使用一个已被桌面端撤销的端口。Android 数据面自身遵循 fail-open，不承诺无中断切换。

use std::net::Ipv4Addr;

use intercept_proxy_application::{
    AndroidNetworkActivation, AndroidRuntimeOwnerMode, AndroidRuntimeOwnerState,
    AndroidRuntimeOwnerTransitionReason, AppError, AppResult,
};

use super::owner::runtime_mode;
use super::{AndroidAdbAdapter, COMMAND_TIMEOUT, PreparedUsbProxyRuntime, ReverseCleanupOutcome};
use crate::adapters::android_adb::command::is_missing_adb_listener_error;

mod lan;
mod preparation;
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
    pub(super) async fn cleanup_owner_reverse(
        &self,
        owner: &intercept_proxy_application::AndroidRuntimeOwnerViewModel,
    ) -> AppResult<()> {
        let active = self
            .owner_state_snapshot_for(&owner.serial)
            .await
            .active_reverse;
        if let Some(active) =
            active.filter(|active| active.serial == owner.serial && active.epoch == owner.epoch)
        {
            let outcome = self
                .remove_reverse_ports(&active.serial, active.ports)
                .await;
            if !outcome.remaining_ports.is_empty() {
                let updated = self
                    .replace_owner_if_epoch(owner.clone(), outcome.remaining_ports.clone())
                    .await?;
                if !updated {
                    return Err(self.runtime_owner_conflict_error(&owner.serial).await);
                }
            }
            return outcome.error.map_or(Ok(()), Err);
        }
        if owner.mode == AndroidRuntimeOwnerMode::AdbReverse {
            self.run_for_serial(&owner.serial, &["reverse", "--remove-all"], COMMAND_TIMEOUT)
                .await?;
        }
        Ok(())
    }

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

    /// 同网段 LAN 可用时直接连接桌面 Listener。部分定制 Android 固件会让
    /// `adb reverse` 在设备端 connect 成功后立即关闭，却从未建立主机侧连接；
    /// 这种情况下应用只能看到 TLS EOF，LAN 路径可以绕过 OEM adbd 缺陷。
    async fn preferred_lan_proxy_host(
        &self,
        serial: &str,
        activation: &AndroidNetworkActivation,
    ) -> Option<Ipv4Addr> {
        self.preferred_lan_proxy_host_strict(serial, activation)
            .await
            .ok()
            .flatten()
    }

    pub(super) async fn preferred_lan_proxy_host_strict(
        &self,
        serial: &str,
        activation: &AndroidNetworkActivation,
    ) -> AppResult<Option<Ipv4Addr>> {
        if activation.proxy_routes.is_empty() {
            return Ok(None);
        }
        let output = self
            .run_for_serial(
                serial,
                &["shell", "ip", "-o", "-4", "addr", "show", "scope", "global"],
                COMMAND_TIMEOUT,
            )
            .await?;
        Ok(parse_device_lan_addresses(&output.stdout)
            .into_iter()
            .find_map(|(device_address, prefix)| {
                let host = self.lan_address.local_ipv4_for(device_address)?;
                lan_endpoint_is_eligible(host, device_address, prefix, &activation.proxy_routes)
                    .then_some(host)
            }))
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
        self.publish_prepared_owner(
            &prepared,
            AndroidRuntimeOwnerState::Uncertain,
            AndroidRuntimeOwnerTransitionReason::ActivationUncertain,
        )
        .await?;
        Err(error
            .retryable("设备最终状态尚未确认；已保留代理映射。请刷新运行状态，或执行停止后重试。"))
    }

    async fn rollback_prepared_network_update(
        &self,
        prepared: PreparedUsbProxyRuntime,
    ) -> AppResult<()> {
        let outcome = if let Some(reverse) = prepared.reverse.as_ref() {
            self.remove_reverse_ports(&reverse.serial, reverse.ports.clone())
                .await
        } else {
            ReverseCleanupOutcome {
                remaining_ports: Vec::new(),
                error: None,
            }
        };
        if outcome.remaining_ports.is_empty() {
            return self.restore_previous_owner(&prepared).await;
        }
        let persistence = self
            .persist_cleanup_required(&prepared, outcome.remaining_ports.clone())
            .await;
        match (outcome.error, persistence) {
            (Some(error), Err(persistence)) => {
                Err(combine_operation_and_cleanup(error, &persistence))
            }
            (Some(error), Ok(())) => Err(error),
            (None, result) => result,
        }
    }

    async fn commit_prepared_network_update(
        &self,
        prepared: PreparedUsbProxyRuntime,
    ) -> AppResult<()> {
        // 清理旧端口前，磁盘必须已经记录新旧端口全集和新 epoch。
        self.publish_prepared_owner(
            &prepared,
            AndroidRuntimeOwnerState::Active,
            AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        )
        .await?;
        let outcome = if let Some(previous) = prepared.previous_reverse.as_ref() {
            self.remove_reverse_ports(&previous.serial, previous.ports.clone())
                .await
        } else {
            ReverseCleanupOutcome {
                remaining_ports: Vec::new(),
                error: None,
            }
        };
        let final_ports = prepared.committed_ports_with(outcome.remaining_ports.clone());
        let mut owner = prepared.owner.clone();
        owner.state = if outcome.remaining_ports.is_empty() {
            AndroidRuntimeOwnerState::Active
        } else {
            AndroidRuntimeOwnerState::CleanupRequired
        };
        owner.transition_reason = if outcome.remaining_ports.is_empty() {
            AndroidRuntimeOwnerTransitionReason::ActivationConfirmed
        } else {
            AndroidRuntimeOwnerTransitionReason::ReverseCleanupRequired
        };
        let serial = owner.serial.clone();
        let persistence = match self.replace_owner_if_epoch(owner, final_ports).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(self.runtime_owner_conflict_error(&serial).await),
            Err(error) => Err(error),
        };
        match (outcome.error, persistence) {
            (Some(error), Err(persistence)) => {
                Err(combine_operation_and_cleanup(error, &persistence))
            }
            (Some(error), Ok(())) => Err(error),
            (None, result) => result,
        }
    }

    async fn persist_cleanup_required(
        &self,
        prepared: &PreparedUsbProxyRuntime,
        remaining_new_ports: Vec<u16>,
    ) -> AppResult<()> {
        let mut owner = prepared.owner.clone();
        owner.state = AndroidRuntimeOwnerState::CleanupRequired;
        owner.transition_reason = AndroidRuntimeOwnerTransitionReason::ReverseCleanupRequired;
        let ports = prepared.cleanup_ports_with(remaining_new_ports);
        let serial = owner.serial.clone();
        match self.replace_owner_if_epoch(owner, ports).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(self.runtime_owner_conflict_error(&serial).await),
            Err(error) => Err(error),
        }
    }
}
