use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use intercept_proxy_application::{
    ANDROID_COMPANION_PACKAGE, AndroidAdbViewModel, AndroidCompanionInstallViewModel,
    AndroidControlPort, AndroidDeviceState, AndroidDeviceViewModel, AndroidNetworkActivation,
    AndroidNetworkStatusViewModel, AndroidPackageViewModel, AppError, AppResult,
};
use serde_json::json;
use tokio::sync::Mutex;

mod command;
mod fingerprint;
mod protocol;
mod reverse;
mod runtime;
mod status;

use command::{AdbCommandRunner, parse_devices, parse_package_version, parse_packages};
use fingerprint::sha256_json;
use protocol::is_socket_unavailable;
use reverse::{combine_operation_and_cleanup, combine_stop_failures, reverse_mapping_present};
use runtime::{
    ActiveReverseOwnership, ActiveRuntimeFacts, DeviceLanAddressProvider, PreparedUsbProxyRuntime,
    ReverseCleanupOutcome,
};
use status::{
    adb_view_model, companion_install_view_model, consent_opened_status,
    control_unavailable_status, normalize_packages,
};

#[cfg(test)]
use command::{AdbOutput, SystemAdbCommandRunner, bundled_companion_apk_candidates};
#[cfg(test)]
use fingerprint::canonical_json;
#[cfg(test)]
use intercept_proxy_application::{AndroidControlTransport, AndroidNetworkState};
#[cfg(test)]
use protocol::{ActivationObservation, classify_activation_status, reconcile_forward_cleanup};
#[cfg(test)]
use reverse::{allocated_reverse_ports, lan_endpoint_is_eligible};
#[cfg(test)]
use std::collections::BTreeMap;
const CONTROL_SOCKET: &str = "intercept_proxy_vpn";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const INSTALL_TIMEOUT: Duration = Duration::from_mins(2);

#[derive(Debug)]
pub struct AndroidAdbAdapter {
    adb_path: Option<PathBuf>,
    companion_apk: Option<PathBuf>,
    selected_serial: RwLock<Option<String>>,
    network_operation: Mutex<()>,
    active_reverse: Mutex<Option<ActiveReverseOwnership>>,
    active_runtime: Mutex<Option<ActiveRuntimeFacts>>,
    runner: Arc<dyn AdbCommandRunner>,
    lan_address: Arc<dyn DeviceLanAddressProvider>,
}

#[async_trait]
impl AndroidControlPort for AndroidAdbAdapter {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        let version = if self.adb_path.is_some() {
            Some(
                self.run(vec!["version".into()], COMMAND_TIMEOUT)
                    .await?
                    .stdout
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            )
        } else {
            None
        };
        Ok(adb_view_model(
            self.adb_path.as_deref(),
            version,
            self.selected_serial
                .read()
                .expect("selected serial lock")
                .clone(),
        ))
    }

    async fn adb_select(&self, serial: String) -> AppResult<AndroidAdbViewModel> {
        let _operation = self.network_operation.lock().await;
        if let Some(ownership) = self.active_reverse.lock().await.as_ref()
            && ownership.serial != serial
        {
            return Err(AppError::new(
                "ANDROID_DEVICE_SWITCH_REQUIRES_STOP",
                format!(
                    "设备 {} 的方案 {} 仍持有透明代理转发；切换设备前必须先停止设备网络接管。",
                    ownership.serial, ownership.profile_id
                ),
            )
            .retryable("请先停止当前设备网络方案，再选择其他设备。"));
        }
        let devices = self.device_list().await?;
        let device = devices
            .iter()
            .find(|device| device.serial == serial)
            .ok_or_else(|| {
                AppError::new(
                    "ANDROID_DEVICE_NOT_FOUND",
                    "所选 Android 设备不在 adb 列表中。",
                )
            })?;
        if device.state != AndroidDeviceState::Device {
            return Err(AppError::new(
                "ANDROID_DEVICE_NOT_READY",
                "所选 Android 设备未在线或未授权。",
            ));
        }
        *self.selected_serial.write().expect("selected serial lock") = Some(serial);
        self.adb_get().await
    }

    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        let output = self
            .run(vec!["devices".into(), "-l".into()], COMMAND_TIMEOUT)
            .await?;
        let selected = self
            .selected_serial
            .read()
            .expect("selected serial lock")
            .clone();
        Ok(parse_devices(&output.stdout, selected.as_deref()))
    }

    async fn package_list(&self) -> AppResult<Vec<AndroidPackageViewModel>> {
        let serial = self.selected_serial()?;
        let output = self
            .run_for_serial(
                &serial,
                &["shell", "pm", "list", "packages", "-U"],
                COMMAND_TIMEOUT,
            )
            .await?;
        Ok(normalize_packages(parse_packages(&output.stdout)))
    }

    async fn package_get(&self, package_name: String) -> AppResult<AndroidPackageViewModel> {
        self.package_list()
            .await?
            .into_iter()
            .find(|package| package.package_name == package_name)
            .ok_or_else(|| {
                AppError::new("ANDROID_PACKAGE_NOT_FOUND", "设备上未找到指定 Android 包。")
            })
    }

    async fn companion_install(&self, update: bool) -> AppResult<AndroidCompanionInstallViewModel> {
        let serial = self.selected_serial()?;
        let apk = self
            .companion_apk
            .as_ref()
            .filter(|path| path.is_file())
            .ok_or_else(|| {
                AppError::new(
                    "ANDROID_COMPANION_APK_NOT_FOUND",
                    "桌面资源中未找到 android-companion APK。",
                )
            })?;
        let mut args = vec!["-s".into(), serial.clone(), "install".into()];
        if update {
            args.push("-r".into());
        }
        args.push(apk.display().to_string());
        let output = self.run(args, INSTALL_TIMEOUT).await?;
        if !output.stdout.lines().any(|line| line.trim() == "Success") {
            return Err(AppError::new(
                "ANDROID_COMPANION_INSTALL_UNVERIFIED",
                "adb install 未返回 Success。",
            ));
        }
        let dump = self
            .run_for_serial(
                &serial,
                &["shell", "dumpsys", "package", ANDROID_COMPANION_PACKAGE],
                COMMAND_TIMEOUT,
            )
            .await?;
        let (version_name, version_code) = parse_package_version(&dump.stdout);
        Ok(companion_install_view_model(
            serial,
            version_name,
            version_code,
        ))
    }

    async fn vpn_open_consent(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = self.selected_serial()?;
        self.run_for_serial(
            &serial,
            &[
                "shell",
                "am",
                "start",
                "-W",
                "-n",
                "com.interceptproxy.vpn/.VpnConsentActivity",
            ],
            COMMAND_TIMEOUT,
        )
        .await?;
        Ok(consent_opened_status(serial))
    }

    async fn network_start(
        &self,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let _operation = self.network_operation.lock().await;
        let prepared = self.prepare_usb_proxy_runtime(&activation).await?;
        let payload =
            json!({"profile": activation.profile, "proxy_runtime": prepared.payload.clone()});
        let accepted = match self.protocol_request("start", payload.clone()).await {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake("start", payload).await
            }
            Err(error) => Err(error),
        };
        match accepted {
            Ok(status) => match self
                .confirm_network_running(&prepared.runtime, status)
                .await
            {
                Ok(confirmed) => {
                    self.finish_prepared_network_update(prepared, Ok(confirmed))
                        .await
                }
                Err(error) if error.view_model.code == "ANDROID_NETWORK_START_FAILED" => {
                    // 设备明确报告 Faulted 时，新 reverse 端口不会再被使用，应立即回滚。
                    // 只有状态查询超时或控制通道中断这类“不确定”结果才保留两代映射。
                    self.finish_prepared_network_update(prepared, Err(error))
                        .await
                }
                Err(error) => self.retain_uncertain_network_update(prepared, error).await,
            },
            Err(error) => {
                self.finish_prepared_network_update(prepared, Err(error))
                    .await
            }
        }
    }

    async fn network_apply(
        &self,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let _operation = self.network_operation.lock().await;
        let prepared = self.prepare_usb_proxy_runtime(&activation).await?;
        let payload =
            json!({"profile": activation.profile, "proxy_runtime": prepared.payload.clone()});
        let accepted = match self.protocol_request("apply", payload.clone()).await {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake("apply", payload).await
            }
            Err(error) => Err(error),
        };
        match accepted {
            Ok(status) => match self
                .confirm_network_running(&prepared.runtime, status)
                .await
            {
                Ok(confirmed) => {
                    self.finish_prepared_network_update(prepared, Ok(confirmed))
                        .await
                }
                Err(error) if error.view_model.code == "ANDROID_NETWORK_START_FAILED" => {
                    self.finish_prepared_network_update(prepared, Err(error))
                        .await
                }
                Err(error) => self.retain_uncertain_network_update(prepared, error).await,
            },
            Err(error) => {
                self.finish_prepared_network_update(prepared, Err(error))
                    .await
            }
        }
    }

    async fn network_runtime_ready(
        &self,
        activation: &AndroidNetworkActivation,
        status: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool> {
        let serial = self.selected_serial()?;
        let active_runtime = self.active_runtime.lock().await.clone();
        let Some(active_runtime) = active_runtime.filter(|runtime| {
            runtime.serial == serial && runtime.profile_id == activation.profile.id
        }) else {
            return Ok(false);
        };
        if status.active_profile_fingerprint.as_deref()
            != Some(active_runtime.profile_fingerprint.as_str())
            || status.active_route_fingerprint.as_deref()
                != Some(active_runtime.route_fingerprint.as_str())
            || status.active_route_count != active_runtime.route_count
        {
            return Ok(false);
        }
        if activation.proxy_routes.is_empty() {
            return Ok(true);
        }
        if !active_runtime.uses_adb_reverse {
            return Ok(true);
        }
        let listing = self
            .run_for_serial(&serial, &["reverse", "--list"], COMMAND_TIMEOUT)
            .await?
            .stdout;
        let listener_ports = active_runtime.listener_ports;
        Ok(listener_ports.iter().all(|(listener_id, device_port)| {
            activation
                .proxy_routes
                .iter()
                .find(|route| route.listener_id == *listener_id)
                .is_some_and(|route| {
                    reverse_mapping_present(&listing, *device_port, route.desktop_listener_port)
                })
        }))
    }

    async fn network_stop(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let _operation = self.network_operation.lock().await;
        let graceful = match self.protocol_request("stop", json!({})).await {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake("stop", json!({})).await
            }
            Err(error) => Err(error),
        };
        let result = match graceful {
            Ok(status) => Ok(status),
            Err(graceful_error) => match self.force_stop_companion().await {
                Ok(status) => Ok(status),
                Err(force_error) => Err(combine_stop_failures(graceful_error, &force_error)),
            },
        };
        let cleanup = self.clear_active_reverse_ports().await;
        match result {
            Ok(status) => cleanup.map(|()| status),
            Err(error) => Err(cleanup.err().map_or(error.clone(), |cleanup_error| {
                combine_operation_and_cleanup(error, &cleanup_error)
            })),
        }
    }

    async fn emergency_restore(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let _operation = self.network_operation.lock().await;
        let force_stop = self.force_stop_companion().await;
        let cleanup = self.clear_active_reverse_ports().await;
        match force_stop {
            Ok(status) => cleanup.map(|()| status),
            Err(error) => Err(cleanup.err().map_or(error.clone(), |cleanup_error| {
                combine_operation_and_cleanup(error, &cleanup_error)
            })),
        }
    }

    async fn network_status(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        match self.protocol_request("status", json!({})).await {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                let serial = self.selected_serial()?;
                let output = self
                    .run_for_serial(
                        &serial,
                        &["shell", "pidof", ANDROID_COMPANION_PACKAGE],
                        COMMAND_TIMEOUT,
                    )
                    .await;
                let running = output
                    .as_ref()
                    .is_ok_and(|output| !output.stdout.trim().is_empty());
                Ok(control_unavailable_status(serial, running))
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
#[path = "android_adb/tests.rs"]
mod tests;
