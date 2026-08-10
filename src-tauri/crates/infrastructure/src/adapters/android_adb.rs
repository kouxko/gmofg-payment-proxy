use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use intercept_proxy_application::{
    ANDROID_COMPANION_PACKAGE, AndroidAdbViewModel, AndroidCompanionInstallViewModel,
    AndroidControlPort, AndroidControlTransport, AndroidDeviceState, AndroidDeviceViewModel,
    AndroidNetworkActivation, AndroidNetworkState, AndroidNetworkStatusViewModel,
    AndroidPackageViewModel, AppError, AppResult,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

mod command;
mod fingerprint;
mod protocol;
mod reverse;

use command::{
    AdbCommandRunner, SystemAdbCommandRunner, discover_adb, discover_companion_apk, parse_devices,
    parse_package_version, parse_packages,
};
use fingerprint::sha256_json;
use protocol::{fallback_unsupported_fields, is_socket_unavailable};
use reverse::{combine_operation_and_cleanup, combine_stop_failures, reverse_mapping_present};

#[cfg(test)]
use command::{AdbOutput, bundled_companion_apk_candidates};
#[cfg(test)]
use fingerprint::canonical_json;
#[cfg(test)]
use protocol::{ActivationObservation, classify_activation_status, reconcile_forward_cleanup};
#[cfg(test)]
use reverse::allocated_reverse_ports;
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveReverseOwnership {
    serial: String,
    profile_id: String,
    ports: Vec<u16>,
}

/// 桌面端为当前 Android start/apply 解析出的运行事实。
///
/// 不能从可持久化 Profile 重新推导该值，因为实际端点包含本次 ADB reverse 端口与
/// DNS 解析结果。桌面进程重启后该事实自然丢失，状态核对会 fail-closed，要求重新
/// apply，而不是假定设备仍连接旧端点。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveRuntimeFacts {
    serial: String,
    profile_id: String,
    profile_fingerprint: String,
    route_fingerprint: String,
    route_count: usize,
    listener_ports: BTreeMap<String, u16>,
}

#[derive(Debug)]
struct PreparedUsbProxyRuntime {
    payload: Value,
    reverse: Option<ActiveReverseOwnership>,
    runtime: ActiveRuntimeFacts,
}

#[derive(Debug)]
struct ReverseCleanupOutcome {
    remaining_ports: Vec<u16>,
    error: Option<AppError>,
}

impl AndroidAdbAdapter {
    #[must_use]
    pub fn new(companion_apk: Option<PathBuf>) -> Self {
        // 优先使用桌面外壳解析的安装资源；无界面测试和其他 Host 再按约定位置回退发现。
        let companion_apk = companion_apk
            .filter(|path| path.is_file())
            .or_else(discover_companion_apk);
        Self {
            adb_path: discover_adb(),
            companion_apk,
            selected_serial: RwLock::new(None),
            network_operation: Mutex::new(()),
            active_reverse: Mutex::new(None),
            active_runtime: Mutex::new(None),
            runner: Arc::new(SystemAdbCommandRunner),
        }
    }
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
        Ok(AndroidAdbViewModel {
            available: self.adb_path.is_some(),
            executable: self
                .adb_path
                .as_ref()
                .map(|path| path.display().to_string()),
            version,
            selected_serial: self
                .selected_serial
                .read()
                .expect("selected serial lock")
                .clone(),
        })
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
        let mut packages = parse_packages(&output.stdout);
        let counts = packages
            .iter()
            .fold(HashMap::<u32, usize>::new(), |mut counts, package| {
                *counts.entry(package.uid).or_default() += 1;
                counts
            });
        for package in &mut packages {
            package.shared_uid =
                (counts.get(&package.uid).copied().unwrap_or_default() > 1).then_some(package.uid);
        }
        packages.sort_by(|left, right| left.package_name.cmp(&right.package_name));
        Ok(packages)
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
        Ok(AndroidCompanionInstallViewModel {
            serial,
            package_name: ANDROID_COMPANION_PACKAGE.into(),
            installed: true,
            version_name,
            version_code,
        })
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
        Ok(AndroidNetworkStatusViewModel {
            serial,
            state: AndroidNetworkState::Unknown,
            state_text: "状态未知".into(),
            ui_tone: intercept_proxy_application::UiTone::Warning,
            verified: false,
            transport: AndroidControlTransport::RescueActivity,
            active_profile_id: None,
            active_profile_fingerprint: None,
            active_route_fingerprint: None,
            active_route_count: 0,
            companion_process_running: None,
            message: "已打开 Android 系统 VPN consent 页面；用户授权结果仅能在设备上确认。".into(),
            unsupported_fields: vec!["vpn_consent_granted".into()],
            stats: None,
        })
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
                Ok(AndroidNetworkStatusViewModel {
                    serial,
                    state: AndroidNetworkState::Unknown,
                    state_text: "状态未知".into(),
                    ui_tone: intercept_proxy_application::UiTone::Warning,
                    verified: false,
                    transport: AndroidControlTransport::Unavailable,
                    active_profile_id: None,
                    active_profile_fingerprint: None,
                    active_route_fingerprint: None,
                    active_route_count: 0,
                    companion_process_running: Some(running),
                    message:
                        "设备端组件未提供控制通道；仅凭进程是否存在无法证明网络接管或弱网数据面状态。"
                            .into(),
                    unsupported_fields: fallback_unsupported_fields(),
                    stats: None,
                })
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
#[path = "android_adb/tests.rs"]
mod tests;
