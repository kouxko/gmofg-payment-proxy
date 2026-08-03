use std::{
    collections::{BTreeMap, HashMap},
    env,
    fmt::Write as _,
    net::TcpListener as StdTcpListener,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use intercept_proxy_application::{
    ANDROID_COMPANION_PACKAGE, ANDROID_CONTROL_MAX_FRAME_BYTES, ANDROID_CONTROL_PROTOCOL_VERSION,
    AndroidAdbViewModel, AndroidCompanionInstallViewModel, AndroidControlPort,
    AndroidControlRequest, AndroidControlResponse, AndroidControlTransport, AndroidDeviceState,
    AndroidDeviceViewModel, AndroidNetworkActivation, AndroidNetworkProfile,
    AndroidNetworkProfileSummary, AndroidNetworkState, AndroidNetworkStatusViewModel,
    AndroidPackageViewModel, AppError, AppResult, OperationResultViewModel,
    encode_android_control_frame,
};
use ring::digest::{SHA256, digest};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::Command,
    sync::Mutex,
    time::timeout,
};

const CONTROL_SOCKET: &str = "intercept_proxy_vpn";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const INSTALL_TIMEOUT: Duration = Duration::from_mins(2);

#[derive(Debug)]
pub struct AndroidAdbAdapter {
    adb_path: Option<PathBuf>,
    companion_apk: Option<PathBuf>,
    selected_serial: RwLock<Option<String>>,
    profiles_path: PathBuf,
    profile_io: Mutex<()>,
    active_reverse_ports: Mutex<Vec<u16>>,
    runner: Arc<dyn AdbCommandRunner>,
}

impl AndroidAdbAdapter {
    #[must_use]
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            adb_path: discover_adb(),
            companion_apk: discover_companion_apk(),
            selected_serial: RwLock::new(None),
            profiles_path: data_dir.as_ref().join("android-network-profiles.json"),
            profile_io: Mutex::new(()),
            active_reverse_ports: Mutex::new(Vec::new()),
            runner: Arc::new(SystemAdbCommandRunner),
        }
    }

    #[cfg(test)]
    fn with_runner(data_dir: &Path, runner: Arc<dyn AdbCommandRunner>) -> Self {
        Self {
            adb_path: Some(PathBuf::from("adb")),
            companion_apk: Some(data_dir.join("android-companion.apk")),
            selected_serial: RwLock::new(None),
            profiles_path: data_dir.join("android-network-profiles.json"),
            profile_io: Mutex::new(()),
            active_reverse_ports: Mutex::new(Vec::new()),
            runner,
        }
    }

    fn adb(&self) -> AppResult<&Path> {
        self.adb_path.as_deref().ok_or_else(|| {
            AppError::new(
                "ANDROID_ADB_NOT_FOUND",
                "未找到系统 adb；桌面应用不会内置 platform-tools。",
            )
            .retryable("请安装 Android platform-tools 并把 adb 加入 PATH。")
        })
    }

    fn selected_serial(&self) -> AppResult<String> {
        self.selected_serial
            .read()
            .expect("selected serial lock")
            .clone()
            .ok_or_else(|| {
                AppError::new(
                    "ANDROID_DEVICE_NOT_SELECTED",
                    "请先选择一台在线 Android 设备。",
                )
            })
    }

    async fn run(&self, args: Vec<String>, duration: Duration) -> AppResult<AdbOutput> {
        let executable = self.adb()?.to_path_buf();
        let runner = Arc::clone(&self.runner);
        let output = timeout(duration, runner.run(&executable, &args))
            .await
            .map_err(|_| AppError::new("ANDROID_ADB_TIMEOUT", "adb 操作超时。"))?
            .map_err(|error| {
                AppError::new("ANDROID_ADB_EXEC_FAILED", format!("无法执行 adb：{error}"))
            })?;
        if !output.success {
            return Err(AppError::new(
                "ANDROID_ADB_COMMAND_FAILED",
                format!(
                    "adb 命令失败：{}",
                    non_empty(&output.stderr, &output.stdout)
                ),
            )
            .retryable("请检查设备是否在线、已授权且 Companion 状态正常。"));
        }
        Ok(output)
    }

    async fn run_for_serial(
        &self,
        serial: &str,
        args: &[&str],
        duration: Duration,
    ) -> AppResult<AdbOutput> {
        let mut owned = vec!["-s".into(), serial.to_owned()];
        owned.extend(args.iter().map(|value| (*value).to_owned()));
        self.run(owned, duration).await
    }

    async fn read_profiles(&self) -> AppResult<BTreeMap<String, AndroidNetworkProfile>> {
        let _guard = self.profile_io.lock().await;
        self.read_profiles_unlocked()
    }

    fn read_profiles_unlocked(&self) -> AppResult<BTreeMap<String, AndroidNetworkProfile>> {
        if !self.profiles_path.exists() {
            return Ok(BTreeMap::new());
        }
        let bytes = std::fs::read(&self.profiles_path).map_err(|error| {
            AppError::new(
                "ANDROID_PROFILE_READ_FAILED",
                format!("无法读取设备网络方案：{error}"),
            )
        })?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(AppError::new(
                "ANDROID_PROFILE_STORE_TOO_LARGE",
                "设备网络方案文件超过 4 MiB 安全上限。",
            ));
        }
        let profiles: Vec<AndroidNetworkProfile> =
            serde_json::from_slice(&bytes).map_err(|error| {
                AppError::new(
                    "ANDROID_PROFILE_STORE_INVALID",
                    format!("设备网络方案文件损坏：{error}"),
                )
            })?;
        let mut by_id = BTreeMap::new();
        for profile in profiles {
            profile.validate()?;
            if by_id.insert(profile.id.clone(), profile).is_some() {
                return Err(AppError::new(
                    "ANDROID_PROFILE_STORE_INVALID",
                    "设备网络方案文件包含重复 ID。",
                ));
            }
        }
        Ok(by_id)
    }

    fn write_profiles_unlocked(
        &self,
        profiles: &BTreeMap<String, AndroidNetworkProfile>,
    ) -> AppResult<()> {
        let bytes =
            serde_json::to_vec_pretty(&profiles.values().collect::<Vec<_>>()).map_err(|error| {
                AppError::new(
                    "ANDROID_PROFILE_WRITE_FAILED",
                    format!("无法序列化设备网络方案：{error}"),
                )
            })?;
        std::fs::write(&self.profiles_path, bytes).map_err(|error| {
            AppError::new(
                "ANDROID_PROFILE_WRITE_FAILED",
                format!("无法保存设备网络方案：{error}"),
            )
        })
    }

    async fn remove_reverse_ports(&self, serial: &str, ports: Vec<u16>) -> AppResult<()> {
        let mut first_error = None;
        for port in ports {
            let result = self
                .run_for_serial(
                    serial,
                    &["reverse", "--remove", &format!("tcp:{port}")],
                    COMMAND_TIMEOUT,
                )
                .await;
            if let Err(error) = result
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn clear_active_reverse_ports(&self, serial: &str) -> AppResult<()> {
        let ports = {
            let mut active = self.active_reverse_ports.lock().await;
            std::mem::take(&mut *active)
        };
        self.remove_reverse_ports(serial, ports).await
    }

    async fn prepare_usb_proxy_runtime(
        &self,
        activation: &AndroidNetworkActivation,
    ) -> AppResult<Value> {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
            net::IpAddr,
        };

        let serial = self.selected_serial()?;
        self.clear_active_reverse_ports(&serial).await?;
        if activation.proxy_routes.is_empty() {
            return Ok(json!({"routes": []}));
        }

        let mut listener_ports = BTreeMap::<String, u16>::new();
        let mut used_device_ports = std::collections::BTreeSet::new();
        let mut created = Vec::new();
        for route in &activation.proxy_routes {
            if listener_ports.contains_key(&route.listener_id) {
                continue;
            }
            let mut hasher = DefaultHasher::new();
            route.listener_id.hash(&mut hasher);
            let mut device_port = 40_000 + u16::try_from(hasher.finish() % 20_000).unwrap_or(0);
            while !used_device_ports.insert(device_port) {
                device_port = if device_port == 59_999 {
                    40_000
                } else {
                    device_port + 1
                };
            }
            let result = self
                .run_for_serial(
                    &serial,
                    &[
                        "reverse",
                        &format!("tcp:{device_port}"),
                        &format!("tcp:{}", route.desktop_listener_port),
                    ],
                    COMMAND_TIMEOUT,
                )
                .await;
            if let Err(error) = result {
                let _ = self.remove_reverse_ports(&serial, created).await;
                return Err(error);
            }
            created.push(device_port);
            listener_ports.insert(route.listener_id.clone(), device_port);
        }

        let mut routes = Vec::with_capacity(activation.proxy_routes.len());
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
                        .collect::<std::collections::BTreeSet<_>>();
                    if addresses.is_empty() {
                        let _ = self.remove_reverse_ports(&serial, created).await;
                        return Err(AppError::new(
                            "ANDROID_PROXY_DESTINATION_RESOLVE_FAILED",
                            format!("透明代理原始域名 {destination} 没有 A/AAAA 记录。"),
                        ));
                    }
                    addresses.into_iter().collect()
                };
            routes.push(json!({
                "listener_id": route.listener_id,
                "original_destination": route.original_destination,
                "original_ports": route.original_ports,
                "resolved_original_ips": resolved_original_ips,
                "proxy_host": "127.0.0.1",
                "proxy_port": listener_ports[&route.listener_id],
            }));
        }
        *self.active_reverse_ports.lock().await = created;
        Ok(json!({"routes": routes}))
    }

    async fn protocol_request(
        &self,
        operation: &str,
        payload: serde_json::Value,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = self.selected_serial()?;
        let request = AndroidControlRequest::new(operation, payload)?;
        let port = reserve_loopback_port()?;
        self.run(
            vec![
                "-s".into(),
                serial.clone(),
                "forward".into(),
                format!("tcp:{port}"),
                format!("localabstract:{CONTROL_SOCKET}"),
            ],
            COMMAND_TIMEOUT,
        )
        .await?;
        let response_serial = serial.clone();
        let result = self.exchange_frame(port, request).await.map(|mut status| {
            // Android 公共 API 无法知道 adb serial；该值属于桌面选择上下文，必须在
            // 完成 request_id/version 校验后由适配器补回，不能相信 Companion 伪造。
            status.serial = response_serial;
            status
        });
        let cleanup = self
            .run_for_serial(
                &serial,
                &["forward", "--remove", &format!("tcp:{port}")],
                COMMAND_TIMEOUT,
            )
            .await;
        reconcile_forward_cleanup(result, cleanup)
    }

    async fn exchange_frame(
        &self,
        port: u16,
        request: AndroidControlRequest,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let mut stream = timeout(
            Duration::from_secs(3),
            TcpStream::connect(("127.0.0.1", port)),
        )
        .await
        .map_err(|_| {
            AppError::new(
                "ANDROID_CONTROL_SOCKET_UNAVAILABLE",
                "设备端组件控制通道连接超时。",
            )
        })?
        .map_err(|error| {
            AppError::new(
                "ANDROID_CONTROL_SOCKET_UNAVAILABLE",
                format!("设备端组件控制通道不可用：{error}"),
            )
        })?;
        let frame = encode_android_control_frame(&request)?;
        timeout(Duration::from_secs(5), stream.write_all(&frame))
            .await
            .map_err(|_| {
                AppError::new("ANDROID_CONTROL_SOCKET_TIMEOUT", "写入设备端控制请求超时。")
            })?
            .map_err(|error| {
                AppError::new(
                    "ANDROID_CONTROL_SOCKET_FAILED",
                    format!("写入设备端控制请求失败：{error}"),
                )
            })?;
        let mut prefix = [0_u8; 4];
        timeout(Duration::from_secs(5), stream.read_exact(&mut prefix))
            .await
            .map_err(|_| {
                AppError::new("ANDROID_CONTROL_SOCKET_TIMEOUT", "读取设备端控制响应超时。")
            })?
            .map_err(|error| {
                AppError::new(
                    "ANDROID_CONTROL_SOCKET_FAILED",
                    format!("读取设备端控制响应失败：{error}"),
                )
            })?;
        let length = u32::from_be_bytes(prefix) as usize;
        if length > ANDROID_CONTROL_MAX_FRAME_BYTES {
            return Err(AppError::new(
                "ANDROID_PROTOCOL_FRAME_TOO_LARGE",
                "设备端响应超过 1 MiB 上限。",
            ));
        }
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload).await.map_err(|error| {
            AppError::new(
                "ANDROID_CONTROL_SOCKET_FAILED",
                format!("设备端响应被截断：{error}"),
            )
        })?;
        let response: AndroidControlResponse =
            serde_json::from_slice(&payload).map_err(|error| {
                AppError::new(
                    "ANDROID_PROTOCOL_JSON_INVALID",
                    format!("设备端响应 JSON 无效：{error}"),
                )
            })?;
        if response.version != ANDROID_CONTROL_PROTOCOL_VERSION
            || response.request_id != request.request_id
        {
            return Err(AppError::new(
                "ANDROID_PROTOCOL_RESPONSE_MISMATCH",
                "设备端响应版本或请求 ID 不匹配。",
            ));
        }
        if !response.ok {
            return Err(AppError::new(
                response
                    .error_code
                    .unwrap_or_else(|| "ANDROID_COMPANION_REJECTED".into()),
                response
                    .error_message
                    .unwrap_or_else(|| "设备端组件拒绝控制请求。".into()),
            ));
        }
        response
            .status
            .map(AndroidNetworkStatusViewModel::with_rust_state_text)
            .ok_or_else(|| {
                AppError::new(
                    "ANDROID_PROTOCOL_RESPONSE_INVALID",
                    "设备端成功响应缺少状态。",
                )
            })
    }

    /// 仅用 Activity 拉起 Companion 进程和 Application 控制服务，随后仍必须通过
    /// 版本化 localabstract 协议重试。这里不携带 Profile、不直接启动或停止 VPN，
    /// 因而 socket 超时不会静默切换到另一套不可验证的控制协议。
    async fn wake_control_server(&self) -> AppResult<()> {
        let serial = self.selected_serial()?;
        self.run_for_serial(
            &serial,
            &[
                "shell",
                "am",
                "start",
                "-W",
                "-n",
                "com.interceptproxy.vpn/.AdbControlActivity",
                "--es",
                "command",
                "wake_control_server",
            ],
            COMMAND_TIMEOUT,
        )
        .await?;
        Ok(())
    }

    async fn protocol_request_after_wake(
        &self,
        operation: &str,
        payload: Value,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        // Activity 唤醒只是帮助 Android 重建 Companion 进程。覆盖安装或 PackageManager
        // 刷新窗口内 `am start` 可能瞬态返回 Error type 3，但 Application 控制 socket
        // 已经可用；最终成功条件仍是下方版本化协议交换，而不是 Activity 启动结果。
        let _wake_result = self.wake_control_server().await;
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(150 * attempt)).await;
            }
            let result = self.protocol_request(operation, payload.clone()).await;
            if !(attempt < 2 && result.as_ref().is_err_and(is_socket_unavailable)) {
                return result;
            }
        }
        unreachable!("bounded control retry always returns on its final attempt")
    }
}

/// 旧版 ADB 在同时连接多台设备时，即使给出 `-s`，`forward tcp:0` 仍可能错误报告
/// “more than one device/emulator”。先让操作系统分配明确端口，再把该端口交给精确
/// serial 的 `adb forward`，可避开这个 ADB server 兼容性问题。
fn reserve_loopback_port() -> AppResult<u16> {
    let listener = StdTcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        AppError::new(
            "ANDROID_ADB_FORWARD_INVALID",
            format!("无法分配 Android 控制通道本地端口：{error}"),
        )
    })?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| {
            AppError::new(
                "ANDROID_ADB_FORWARD_INVALID",
                format!("无法读取 Android 控制通道本地端口：{error}"),
            )
        })
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
            let dump = self
                .run_for_serial(
                    &serial,
                    &["shell", "dumpsys", "package", &package.package_name],
                    COMMAND_TIMEOUT,
                )
                .await?;
            package.signing_sha256 = parse_signing_sha256(&dump.stdout);
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
            companion_process_running: None,
            message: "已打开 Android 系统 VPN consent 页面；用户授权结果仅能在设备上确认。".into(),
            unsupported_fields: vec!["vpn_consent_granted".into()],
            stats: None,
        })
    }

    async fn profile_list(&self) -> AppResult<Vec<AndroidNetworkProfileSummary>> {
        Ok(self
            .read_profiles()
            .await?
            .values()
            .map(AndroidNetworkProfileSummary::from)
            .collect())
    }

    async fn profile_get(&self, profile_id: String) -> AppResult<AndroidNetworkProfile> {
        self.read_profiles()
            .await?
            .remove(&profile_id)
            .ok_or_else(|| {
                AppError::new("ANDROID_PROFILE_NOT_FOUND", "未找到指定设备网络方案。")
                    .entity(profile_id)
            })
    }

    async fn profile_save(
        &self,
        profile: AndroidNetworkProfile,
    ) -> AppResult<AndroidNetworkProfile> {
        profile.validate()?;
        let _guard = self.profile_io.lock().await;
        let mut profiles = self.read_profiles_unlocked()?;
        profiles.insert(profile.id.clone(), profile.clone());
        self.write_profiles_unlocked(&profiles)?;
        Ok(profile)
    }

    async fn profile_delete(&self, profile_id: String) -> AppResult<OperationResultViewModel> {
        let _guard = self.profile_io.lock().await;
        let mut profiles = self.read_profiles_unlocked()?;
        if profiles.remove(&profile_id).is_none() {
            return Err(
                AppError::new("ANDROID_PROFILE_NOT_FOUND", "未找到指定设备网络方案。")
                    .entity(profile_id),
            );
        }
        self.write_profiles_unlocked(&profiles)?;
        Ok(OperationResultViewModel::success("设备网络方案已删除。"))
    }

    async fn network_start(
        &self,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let proxy_runtime = self.prepare_usb_proxy_runtime(&activation).await?;
        let payload = json!({"profile": activation.profile, "proxy_runtime": proxy_runtime});
        let result = match self.protocol_request("start", payload.clone()).await {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake("start", payload).await
            }
            Err(error) => Err(error),
        };
        if result.is_err() {
            let serial = self.selected_serial()?;
            let _ = self.clear_active_reverse_ports(&serial).await;
        }
        result
    }

    async fn network_apply(
        &self,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let proxy_runtime = self.prepare_usb_proxy_runtime(&activation).await?;
        let payload = json!({"profile": activation.profile, "proxy_runtime": proxy_runtime});
        let result = match self.protocol_request("apply", payload.clone()).await {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake("apply", payload).await
            }
            Err(error) => Err(error),
        };
        if result.is_err() {
            let serial = self.selected_serial()?;
            let _ = self.clear_active_reverse_ports(&serial).await;
        }
        result
    }

    async fn network_stop(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let result = match self.protocol_request("stop", json!({})).await {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake("stop", json!({})).await
            }
            Err(error) => Err(error),
        };
        let serial = self.selected_serial()?;
        let cleanup = self.clear_active_reverse_ports(&serial).await;
        match result {
            Ok(status) => cleanup.map(|()| status),
            Err(error) => Err(error),
        }
    }

    async fn emergency_restore(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = self.selected_serial()?;
        self.run_for_serial(
            &serial,
            &["shell", "am", "force-stop", ANDROID_COMPANION_PACKAGE],
            COMMAND_TIMEOUT,
        )
        .await?;
        let _ = self.clear_active_reverse_ports(&serial).await;
        Ok(AndroidNetworkStatusViewModel {
            serial,
            state: AndroidNetworkState::Stopped,
            state_text: "已停止".into(),
            ui_tone: intercept_proxy_application::UiTone::Neutral,
            verified: true,
            transport: AndroidControlTransport::AdbForceStop,
            active_profile_id: None,
            companion_process_running: Some(false),
            message: "设备端组件进程已被系统强制停止；TUN 文件描述符随进程关闭，设备网络已恢复为故障放行。".into(),
            unsupported_fields: vec!["last_profile_id".into(), "packet_stats".into()],
            stats: None,
        })
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

#[async_trait]
trait AdbCommandRunner: Send + Sync + std::fmt::Debug {
    async fn run(&self, executable: &Path, args: &[String]) -> std::io::Result<AdbOutput>;
}

#[derive(Debug)]
struct SystemAdbCommandRunner;

#[async_trait]
impl AdbCommandRunner for SystemAdbCommandRunner {
    async fn run(&self, executable: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        let mut command = Command::new(executable);
        command.args(args).kill_on_drop(true);
        let output = command.output().await?;
        Ok(AdbOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Clone, Debug)]
struct AdbOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

fn discover_adb() -> Option<PathBuf> {
    let executable = if cfg!(windows) { "adb.exe" } else { "adb" };
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|directory| directory.join(executable)));
    }
    for variable in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(root) = env::var_os(variable) {
            candidates.push(PathBuf::from(root).join("platform-tools").join(executable));
        }
    }
    if let Some(home) = env::var_os("HOME") {
        candidates.push(
            PathBuf::from(home)
                .join("Library/Android/sdk/platform-tools")
                .join(executable),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn discover_companion_apk() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("INTERCEPT_PROXY_ANDROID_COMPANION_APK") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(executable) = env::current_exe() {
        candidates.extend(bundled_companion_apk_candidates(&executable));
    }
    candidates.push(PathBuf::from(
        "android-companion/app/build/outputs/apk/release/app-release.apk",
    ));
    candidates.push(PathBuf::from(
        "android-companion/app/build/outputs/apk/debug/app-debug.apk",
    ));
    candidates.into_iter().find(|path| path.is_file())
}

/// 返回桌面安装包中 Companion APK 的平台候选位置。
///
/// Tauri 在 Windows/Linux 中把资源放在可执行文件旁的 `resources` 目录；macOS
/// `.app` 会保留配置中的 `resources/` 前缀，因此实际位置是
/// `Contents/Resources/resources/android-companion.apk`。路径解析留在基础设施层，
/// Android 业务用例不需要了解桌面安装包结构。
fn bundled_companion_apk_candidates(executable: &Path) -> Vec<PathBuf> {
    let Some(directory) = executable.parent() else {
        return Vec::new();
    };
    vec![
        directory.join("resources/android-companion.apk"),
        directory.join("../Resources/resources/android-companion.apk"),
        directory.join("../Resources/android-companion.apk"),
        directory.join("android-companion.apk"),
    ]
}

fn parse_devices(output: &str, selected: Option<&str>) -> Vec<AndroidDeviceViewModel> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?.to_owned();
            let raw_state = fields.next()?;
            if serial.starts_with('*') {
                return None;
            }
            let properties = fields
                .filter_map(|field| field.split_once(':'))
                .collect::<HashMap<_, _>>();
            Some(AndroidDeviceViewModel {
                selected: selected == Some(serial.as_str()),
                serial,
                state: match raw_state {
                    "device" => AndroidDeviceState::Device,
                    "offline" => AndroidDeviceState::Offline,
                    "unauthorized" => AndroidDeviceState::Unauthorized,
                    _ => AndroidDeviceState::Other,
                },
                product: properties.get("product").map(|value| (*value).to_owned()),
                model: properties.get("model").map(|value| (*value).to_owned()),
                device: properties.get("device").map(|value| (*value).to_owned()),
                transport_id: properties
                    .get("transport_id")
                    .map(|value| (*value).to_owned()),
            })
        })
        .collect()
}

fn parse_packages(output: &str) -> Vec<AndroidPackageViewModel> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("package:")?;
            let (package_name, uid) = line.rsplit_once(" uid:")?;
            Some(AndroidPackageViewModel {
                package_name: package_name.to_owned(),
                uid: uid.trim().parse().ok()?,
                signing_sha256: None,
                shared_uid: None,
            })
        })
        .collect()
}

fn parse_signing_sha256(output: &str) -> Option<String> {
    if let Some(digest) = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("SHA-256 digest:"))
    {
        let value = digest.trim();
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    let start = output.find("signatures:[")? + "signatures:[".len();
    let end = output[start..].find(']')? + start;
    let fingerprints = output[start..end]
        .split(',')
        .filter_map(|value| decode_hex(value.trim()))
        .map(|certificate| format_digest(digest(&SHA256, &certificate).as_ref()))
        .collect::<Vec<_>>();
    (!fingerprints.is_empty()).then(|| fingerprints.join("+"))
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    let compact = value.replace(':', "");
    if !compact.len().is_multiple_of(2) || !compact.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    (0..compact.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&compact[index..index + 2], 16).ok())
        .collect()
}

fn format_digest(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn parse_package_version(output: &str) -> (Option<String>, Option<String>) {
    let version_name = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("versionName=").map(str::to_owned));
    let version_code = output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("versionCode=")
            .and_then(|value| value.split_whitespace().next())
            .map(str::to_owned)
    });
    (version_name, version_code)
}

fn is_socket_unavailable(error: &AppError) -> bool {
    matches!(
        error.view_model.code.as_str(),
        "ANDROID_CONTROL_SOCKET_UNAVAILABLE"
            | "ANDROID_CONTROL_SOCKET_TIMEOUT"
            | "ANDROID_CONTROL_SOCKET_FAILED"
    )
}

fn fallback_unsupported_fields() -> Vec<String> {
    vec![
        "vpn_running".into(),
        "active_profile_id".into(),
        "packet_stats".into(),
        "data_plane_available".into(),
    ]
}

fn non_empty<'a>(preferred: &'a str, fallback: &'a str) -> &'a str {
    if preferred.trim().is_empty() {
        fallback.trim()
    } else {
        preferred.trim()
    }
}

fn reconcile_forward_cleanup<T>(
    result: AppResult<T>,
    cleanup: AppResult<AdbOutput>,
) -> AppResult<T> {
    match (result, cleanup) {
        (Ok(value), Ok(_)) => Ok(value),
        (Ok(_), Err(cleanup_error)) => Err(AppError::new(
            "ANDROID_ADB_FORWARD_CLEANUP_FAILED",
            format!(
                "设备控制请求已完成，但 adb forward 清理失败：{}",
                cleanup_error.view_model.message
            ),
        )
        .retryable("请检查 adb forward 列表并重试。")),
        (Err(error), Ok(_)) => Err(error),
        (Err(mut error), Err(cleanup_error)) => {
            let _ = write!(
                error.view_model.message,
                "；同时 adb forward 清理失败：{}",
                cleanup_error.view_model.message
            );
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use intercept_proxy_application::{
        AndroidProxyRouteActivation, AndroidTargetApplication, WeakNetworkProfile,
    };
    use intercept_proxy_domain::ListenerId;

    #[test]
    fn parses_devices_packages_shared_uid_inputs_and_certificate_digest() {
        let devices = parse_devices(
            "List of devices attached\nSER123 device product:a920 model:A920MAX device:a920 transport_id:7\nOFF offline\n",
            Some("SER123"),
        );
        assert_eq!(devices.len(), 2);
        assert!(devices[0].selected);
        assert_eq!(devices[0].model.as_deref(), Some("A920MAX"));

        let packages = parse_packages(
            "package:com.example.one uid:10123\npackage:com.example.two uid:10123\n",
        );
        assert_eq!(packages[0].uid, 10_123);
        let certificate_hex = "00".repeat(32);
        let dump = format!("signatures=PackageSignatures{{ signatures:[{certificate_hex}] }}");
        assert_eq!(parse_signing_sha256(&dump).unwrap().split(':').count(), 32);
    }

    #[test]
    fn packaged_apk_path_is_owned_by_adapter_not_command_input() {
        let temp = tempfile::tempdir().unwrap();
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
        assert_eq!(
            adapter.companion_apk.as_deref(),
            Some(temp.path().join("android-companion.apk").as_path())
        );
    }

    #[test]
    fn packaged_apk_candidates_include_the_tauri_macos_resource_layout() {
        let executable =
            Path::new("/Applications/Intercept Proxy.app/Contents/MacOS/intercept-proxy");
        let candidates = bundled_companion_apk_candidates(executable);

        assert!(candidates.contains(&PathBuf::from(
            "/Applications/Intercept Proxy.app/Contents/MacOS/../Resources/resources/android-companion.apk",
        )));
    }

    #[derive(Debug)]
    struct FakeRunner;

    #[async_trait]
    impl AdbCommandRunner for FakeRunner {
        async fn run(&self, _: &Path, _: &[String]) -> std::io::Result<AdbOutput> {
            Ok(AdbOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    #[derive(Debug, Default)]
    struct RecordingRunner {
        calls: std::sync::Mutex<Vec<Vec<String>>>,
    }

    #[async_trait]
    impl AdbCommandRunner for RecordingRunner {
        async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
            self.calls.lock().unwrap().push(args.to_vec());
            Ok(AdbOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    #[tokio::test]
    async fn usb_runtime_creates_reverse_and_keeps_endpoint_out_of_profile() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(RecordingRunner::default());
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
        *adapter.selected_serial.write().unwrap() = Some("SER123".into());
        let listener_id = ListenerId::new();
        let profile = AndroidNetworkProfile {
            id: "route-profile".into(),
            name: "路由".into(),
            target_applications: vec![AndroidTargetApplication {
                package_name: "com.example.target".into(),
                signing_sha256: "AA".into(),
                uid: 10_001,
                display_name: None,
            }],
            destination_targets: Vec::new(),
            proxy_routes: vec![intercept_proxy_domain::AndroidProxyRoute {
                destination: "203.0.113.10".into(),
                ports: vec![16_127],
                listener_id,
            }],
            confirmed_shared_uids: std::collections::BTreeSet::default(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        };
        let activation = AndroidNetworkActivation {
            profile: profile.clone(),
            proxy_routes: vec![AndroidProxyRouteActivation {
                listener_id: listener_id.to_string(),
                original_destination: "203.0.113.10".into(),
                original_ports: vec![16_127],
                desktop_listener_port: 26_127,
            }],
        };

        let runtime = adapter
            .prepare_usb_proxy_runtime(&activation)
            .await
            .unwrap();
        let route = &runtime["routes"][0];
        assert_eq!(route["proxy_host"], "127.0.0.1");
        assert_eq!(route["original_destination"], "203.0.113.10");
        assert!(route["proxy_port"].as_u64().is_some());
        assert!(
            !serde_json::to_value(profile)
                .unwrap()
                .to_string()
                .contains("proxy_host")
        );
        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|args| {
            args.windows(2)
                .any(|pair| pair[0] == "reverse" && pair[1].starts_with("tcp:"))
                && args.last() == Some(&"tcp:26127".to_owned())
        }));
    }

    #[test]
    fn forward_cleanup_failure_is_observable_after_successful_request() {
        let cleanup = Err(AppError::new(
            "ANDROID_ADB_COMMAND_FAILED",
            "cannot remove forward",
        ));
        let error = reconcile_forward_cleanup(Ok("response"), cleanup).unwrap_err();

        assert_eq!(error.view_model.code, "ANDROID_ADB_FORWARD_CLEANUP_FAILED");
        assert!(error.view_model.message.contains("cannot remove forward"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_timeout_terminates_the_spawned_process() {
        let temp = tempfile::tempdir().unwrap();
        let pid_path = temp.path().join("adb-child.pid");
        let script = format!("echo $$ > '{}'; exec sleep 30", pid_path.display());
        let mut adapter =
            AndroidAdbAdapter::with_runner(temp.path(), Arc::new(SystemAdbCommandRunner));
        adapter.adb_path = Some(PathBuf::from("/bin/sh"));

        let error = adapter
            .run(vec!["-c".into(), script], Duration::from_millis(100))
            .await
            .unwrap_err();
        assert_eq!(error.view_model.code, "ANDROID_ADB_TIMEOUT");
        let pid = std::fs::read_to_string(&pid_path).unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;
        let still_running = std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        assert!(!still_running, "timed-out adb child {pid} is still running");
    }
}
