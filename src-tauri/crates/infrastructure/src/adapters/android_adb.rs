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
    AndroidDeviceViewModel, AndroidNetworkActivation, AndroidNetworkState,
    AndroidNetworkStatusViewModel, AndroidPackageViewModel, AndroidProxyRouteActivation, AppError,
    AppResult, encode_android_control_frame,
};
use serde::Serialize;
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

fn sha256_json(value: &impl Serialize) -> AppResult<String> {
    let value = serde_json::to_value(value).map_err(|error| {
        AppError::new(
            "ANDROID_RUNTIME_FINGERPRINT_FAILED",
            format!("无法生成设备网络运行指纹：{error}"),
        )
    })?;
    let bytes = canonical_json(&value).into_bytes();
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    let mut encoded = String::with_capacity(digest.as_ref().len() * 2);
    for byte in digest.as_ref() {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(encoded)
}

/// 生成跨 Rust/Android 稳定的 JSON 表示，用于证明 Profile 与运行路由属于同一次激活。
///
/// 不能直接比较两端 JSON writer 的输出：Android 会把 `/` 写成 `\/`，而
/// `serde_json` 保留 `/`。这里明确排序对象键，并只使用 JSON 标准要求的字符串转义；
/// Android Companion 实现同一规则，因此 CIDR、URL 等合法字符串不会产生假冲突。
fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => canonical_json_string(value),
        serde_json::Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        serde_json::Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);
            format!(
                "{{{}}}",
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        format!("{}:{}", canonical_json_string(key), canonical_json(value))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn canonical_json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

#[derive(Debug)]
pub struct AndroidAdbAdapter {
    adb_path: Option<PathBuf>,
    companion_apk: Option<PathBuf>,
    selected_serial: RwLock<Option<String>>,
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
}

#[derive(Debug)]
struct ReverseCleanupOutcome {
    remaining_ports: Vec<u16>,
    error: Option<AppError>,
}

impl AndroidAdbAdapter {
    #[must_use]
    pub fn new(_data_dir: impl AsRef<Path>) -> Self {
        Self {
            adb_path: discover_adb(),
            companion_apk: discover_companion_apk(),
            selected_serial: RwLock::new(None),
            active_reverse: Mutex::new(None),
            active_runtime: Mutex::new(None),
            runner: Arc::new(SystemAdbCommandRunner),
        }
    }

    #[cfg(test)]
    fn with_runner(data_dir: &Path, runner: Arc<dyn AdbCommandRunner>) -> Self {
        Self {
            adb_path: Some(PathBuf::from("adb")),
            companion_apk: Some(data_dir.join("android-companion.apk")),
            selected_serial: RwLock::new(None),
            active_reverse: Mutex::new(None),
            active_runtime: Mutex::new(None),
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

    /// 建立或清理 `adb forward` 时始终只操作用户明确选择的 serial。
    ///
    /// `adb reconnect offline` 会修改 ADB server 中所有离线 transport，可能影响另一个
    /// 正在调试的设备。遇到陈旧 transport 时返回可观察错误，由显式的设备刷新流程恢复。
    async fn run_forward_for_serial(&self, serial: &str, args: &[&str]) -> AppResult<AdbOutput> {
        match self.run_for_serial(serial, args, COMMAND_TIMEOUT).await {
            Err(error) if is_stale_adb_transport_error(&error) => Err(AppError::new(
                "ANDROID_ADB_SELECTED_TRANSPORT_STALE",
                format!("选中设备 {serial} 的 ADB 转发被陈旧 transport 干扰；未修改其他设备连接。"),
            )
            .retryable("请刷新设备列表或显式清理离线 ADB 连接后重试。")),
            result => result,
        }
    }

    async fn remove_reverse_ports(&self, serial: &str, ports: Vec<u16>) -> ReverseCleanupOutcome {
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
            if let Err(error) = result {
                remaining_ports.push(port);
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        ReverseCleanupOutcome {
            remaining_ports,
            error: first_error,
        }
    }

    async fn clear_active_reverse_ports(&self) -> AppResult<()> {
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
                let cleanup = self.remove_reverse_ports(serial, created).await;
                if !cleanup.remaining_ports.is_empty() {
                    *self.active_reverse.lock().await = Some(ActiveReverseOwnership {
                        serial: serial.to_owned(),
                        profile_id: activation.profile.id.clone(),
                        ports: cleanup.remaining_ports,
                    });
                }
                return Err(cleanup.error.map_or(error.clone(), |cleanup_error| {
                    combine_operation_and_cleanup(error, &cleanup_error)
                }));
            }
            created.push(*device_port);
        }
        Ok(created)
    }

    async fn prepare_usb_proxy_runtime(
        &self,
        activation: &AndroidNetworkActivation,
    ) -> AppResult<Value> {
        use std::net::IpAddr;

        let serial = self.selected_serial()?;
        let profile_fingerprint = sha256_json(&activation.profile)?;
        let route_count = activation.proxy_routes.len();
        self.clear_active_reverse_ports().await?;
        if activation.proxy_routes.is_empty() {
            let routes = Vec::<Value>::new();
            let route_fingerprint = sha256_json(&routes)?;
            *self.active_runtime.lock().await = Some(ActiveRuntimeFacts {
                serial,
                profile_id: activation.profile.id.clone(),
                profile_fingerprint: profile_fingerprint.clone(),
                route_fingerprint: route_fingerprint.clone(),
                route_count,
            });
            return Ok(json!({
                "routes": [],
                "route_source": activation.proxy_routes,
                "profile_fingerprint": profile_fingerprint,
                "route_fingerprint": route_fingerprint,
                "route_count": route_count,
            }));
        }

        let listener_ports = allocated_reverse_ports(&activation.proxy_routes);
        // 先完成所有可能失败的 DNS 解析，再创建 `adb reverse`。
        // 这样解析错误不会留下尚未登记所有权、也无法由 stop 清理的设备端映射。
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
                        .collect::<std::collections::BTreeSet<_>>();
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

        let created = self
            .create_reverse_mappings(&serial, activation, &listener_ports)
            .await?;

        let mut routes = Vec::with_capacity(resolved_routes.len());
        for (route, resolved_original_ips) in resolved_routes {
            routes.push(json!({
                "listener_id": route.listener_id,
                "original_destination": route.original_destination,
                "original_ports": route.original_ports,
                "resolved_original_ips": resolved_original_ips,
                "proxy_host": "127.0.0.1",
                "proxy_port": listener_ports[&route.listener_id],
            }));
        }
        let route_fingerprint = sha256_json(&routes)?;
        *self.active_reverse.lock().await = Some(ActiveReverseOwnership {
            serial: serial.clone(),
            profile_id: activation.profile.id.clone(),
            ports: created,
        });
        *self.active_runtime.lock().await = Some(ActiveRuntimeFacts {
            serial,
            profile_id: activation.profile.id.clone(),
            profile_fingerprint: profile_fingerprint.clone(),
            route_fingerprint: route_fingerprint.clone(),
            route_count,
        });
        Ok(json!({
            "routes": routes,
            "route_source": activation.proxy_routes,
            "profile_fingerprint": profile_fingerprint,
            "route_fingerprint": route_fingerprint,
            "route_count": route_count,
        }))
    }

    async fn protocol_request(
        &self,
        operation: &str,
        payload: serde_json::Value,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let serial = self.selected_serial()?;
        let request = AndroidControlRequest::new(operation, payload)?;
        let port = reserve_loopback_port()?;
        self.run_forward_for_serial(
            &serial,
            &[
                "forward",
                &format!("tcp:{port}"),
                &format!("localabstract:{CONTROL_SOCKET}"),
            ],
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
            .run_forward_for_serial(&serial, &["forward", "--remove", &format!("tcp:{port}")])
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

/// 先让操作系统分配明确端口，避免 `adb forward tcp:0` 在多设备环境中自行选错
/// transport。ADB server 的陈旧离线 transport 由 `run_forward_for_serial` 另行恢复。
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

/// 依据入口 ID 推导稳定的设备侧端口。
///
/// ADB 或桌面进程重启后，内存中的端口记录会丢失；稳定映射使 Rust 可以检查
/// 运行中的 VPN 是否仍有到桌面代理的 `adb reverse` 通道。
fn allocated_reverse_ports(routes: &[AndroidProxyRouteActivation]) -> BTreeMap<String, u16> {
    use std::{
        collections::{BTreeSet, hash_map::DefaultHasher},
        hash::{Hash, Hasher},
    };

    let mut listener_ports = BTreeMap::new();
    let mut used_device_ports = BTreeSet::new();
    for route in routes {
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
        listener_ports.insert(route.listener_id.clone(), device_port);
    }
    listener_ports
}

fn reverse_mapping_present(listing: &str, device_port: u16, desktop_port: u16) -> bool {
    let device = format!("tcp:{device_port}");
    let desktop = format!("tcp:{desktop_port}");
    listing.lines().any(|line| {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        fields
            .windows(2)
            .any(|pair| pair[0] == device && pair[1] == desktop)
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
            let cleanup = self.clear_active_reverse_ports().await;
            return reconcile_operation_cleanup(result, cleanup);
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
            let cleanup = self.clear_active_reverse_ports().await;
            return reconcile_operation_cleanup(result, cleanup);
        }
        result
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
        let listener_ports = allocated_reverse_ports(&activation.proxy_routes);
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
        let result = match self.protocol_request("stop", json!({})).await {
            Ok(status) => Ok(status),
            Err(error) if is_socket_unavailable(&error) => {
                self.protocol_request_after_wake("stop", json!({})).await
            }
            Err(error) => Err(error),
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
        let serial = self.selected_serial()?;
        self.run_for_serial(
            &serial,
            &["shell", "am", "force-stop", ANDROID_COMPANION_PACKAGE],
            COMMAND_TIMEOUT,
        )
        .await?;
        self.clear_active_reverse_ports().await?;
        Ok(AndroidNetworkStatusViewModel {
            serial,
            state: AndroidNetworkState::Stopped,
            state_text: "已停止".into(),
            ui_tone: intercept_proxy_application::UiTone::Neutral,
            verified: true,
            transport: AndroidControlTransport::AdbForceStop,
            active_profile_id: None,
            active_profile_fingerprint: None,
            active_route_fingerprint: None,
            active_route_count: 0,
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
                shared_uid: None,
            })
        })
        .collect()
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

fn is_stale_adb_transport_error(error: &AppError) -> bool {
    let message = error.view_model.message.to_ascii_lowercase();
    message.contains("more than one device/emulator")
        || message.contains("more than one device or emulator")
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

fn combine_operation_and_cleanup(mut operation: AppError, cleanup: &AppError) -> AppError {
    let _ = write!(
        operation.view_model.message,
        "；同时 adb reverse 清理失败：{}",
        cleanup.view_model.message
    );
    operation.view_model.retryable = true;
    operation.view_model.suggested_action =
        Some("请保持设备在线并再次停止设备网络接管或执行紧急恢复，以重试清理残留映射。".into());
    operation
}

fn reconcile_operation_cleanup<T>(operation: AppResult<T>, cleanup: AppResult<()>) -> AppResult<T> {
    match (operation, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
        (Err(operation_error), Ok(())) => Err(operation_error),
        (Err(operation_error), Err(cleanup_error)) => Err(combine_operation_and_cleanup(
            operation_error,
            &cleanup_error,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use intercept_proxy_application::{
        AndroidNetworkProfile, AndroidProxyRouteActivation, AndroidTargetApplication,
        WeakNetworkProfile,
    };
    use intercept_proxy_domain::ListenerId;

    #[test]
    fn canonical_fingerprint_matches_android_for_cidr_and_url() {
        let value = serde_json::json!({
            "destination_targets": [{
                "address": "10.0.0.0/8",
                "ports": [16_127]
            }],
            "server_url": "https://example.test:16127/path"
        });

        assert_eq!(
            canonical_json(&value),
            r#"{"destination_targets":[{"address":"10.0.0.0/8","ports":[16127]}],"server_url":"https://example.test:16127/path"}"#
        );
        assert_eq!(
            sha256_json(&value).unwrap(),
            "1b9889227509e4d1dca893ffc0d023e82e96258b84c34f03f4d550361a47db1a"
        );
    }

    #[test]
    fn parses_devices_and_packages_with_shared_uid_inputs() {
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
    }

    #[test]
    fn reverse_runtime_mapping_accepts_adb_serial_prefix() {
        let routes = vec![AndroidProxyRouteActivation {
            listener_id: "listener-a".into(),
            original_destination: "203.0.113.10".into(),
            original_ports: vec![16_127],
            desktop_listener_port: 26_127,
        }];
        let device_port = allocated_reverse_ports(&routes)["listener-a"];

        assert!(reverse_mapping_present(
            &format!("SER123 tcp:{device_port} tcp:26127\n"),
            device_port,
            26_127,
        ));
        assert!(!reverse_mapping_present(
            &format!("SER123 tcp:{device_port} tcp:16627\n"),
            device_port,
            26_127,
        ));
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

    #[derive(Debug)]
    struct SequenceRunner {
        calls: std::sync::Mutex<Vec<Vec<String>>>,
        outputs: std::sync::Mutex<std::collections::VecDeque<AdbOutput>>,
    }

    #[async_trait]
    impl AdbCommandRunner for SequenceRunner {
        async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
            self.calls.lock().unwrap().push(args.to_vec());
            Ok(self
                .outputs
                .lock()
                .unwrap()
                .pop_front()
                .expect("测试必须为每次 adb 调用提供结果"))
        }
    }

    #[tokio::test]
    async fn wake_control_server_does_not_wait_for_activity_completion() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(RecordingRunner::default());
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
        *adapter.selected_serial.write().unwrap() = Some("2740072778".into());

        adapter.wake_control_server().await.unwrap();

        assert_eq!(
            *runner.calls.lock().unwrap(),
            vec![vec![
                "-s",
                "2740072778",
                "shell",
                "am",
                "start",
                "-n",
                "com.interceptproxy.vpn/.AdbControlActivity",
                "--es",
                "command",
                "wake_control_server",
            ]]
        );
    }

    #[tokio::test]
    async fn package_inventory_does_not_read_apk_signatures() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(SequenceRunner {
            calls: std::sync::Mutex::new(Vec::new()),
            outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
                success: true,
                stdout: "package:com.example.client uid:10001\n".into(),
                stderr: String::new(),
            }])),
        });
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
        *adapter.selected_serial.write().unwrap() = Some("2740072778".into());

        let packages = adapter.package_list().await.unwrap();

        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].package_name, "com.example.client");
        assert_eq!(
            *runner.calls.lock().unwrap(),
            vec![vec![
                "-s",
                "2740072778",
                "shell",
                "pm",
                "list",
                "packages",
                "-U",
            ]]
        );
    }

    #[tokio::test]
    async fn stale_multi_device_forward_fails_without_mutating_other_transports() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(SequenceRunner {
            calls: std::sync::Mutex::new(Vec::new()),
            outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
                success: false,
                stdout: String::new(),
                stderr: "adb: error: more than one device/emulator".into(),
            }])),
        });
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());

        let error = adapter
            .run_forward_for_serial(
                "2740072778",
                &["forward", "tcp:47123", "localabstract:intercept_proxy_vpn"],
            )
            .await
            .expect_err("陈旧 transport 必须由显式设备刷新恢复");

        assert_eq!(
            error.view_model.code,
            "ANDROID_ADB_SELECTED_TRANSPORT_STALE"
        );
        assert_eq!(
            *runner.calls.lock().unwrap(),
            vec![vec![
                "-s",
                "2740072778",
                "forward",
                "tcp:47123",
                "localabstract:intercept_proxy_vpn",
            ]]
        );
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
        assert_eq!(runtime["route_count"], 1);
        assert_eq!(
            runtime["route_source"][0]["listener_id"],
            listener_id.to_string()
        );
        assert!(runtime["profile_fingerprint"].as_str().is_some());
        assert!(runtime["route_fingerprint"].as_str().is_some());
        assert_eq!(
            runtime["route_fingerprint"],
            sha256_json(&runtime["routes"]).unwrap()
        );
        assert_ne!(
            runtime["route_fingerprint"],
            sha256_json(&runtime["route_source"]).unwrap()
        );
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

    #[tokio::test]
    async fn normalized_route_fingerprint_changes_when_runtime_endpoint_changes() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(RecordingRunner::default());
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner);
        *adapter.selected_serial.write().unwrap() = Some("SER123".into());
        let listener_id = ListenerId::new();
        let activation = AndroidNetworkActivation {
            profile: AndroidNetworkProfile {
                id: "endpoint-fingerprint".into(),
                name: "endpoint-fingerprint".into(),
                target_applications: Vec::new(),
                destination_targets: Vec::new(),
                proxy_routes: vec![intercept_proxy_domain::AndroidProxyRoute {
                    destination: "203.0.113.10".into(),
                    ports: vec![16_127],
                    listener_id,
                }],
                confirmed_shared_uids: std::collections::BTreeSet::default(),
                auto_resume_after_reboot: false,
                weak_network: WeakNetworkProfile::default(),
            },
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
        let declared = runtime["route_fingerprint"].as_str().unwrap();
        let mut wrong_routes = runtime["routes"].clone();
        wrong_routes[0]["proxy_port"] = json!(49_999);

        assert_ne!(declared, sha256_json(&wrong_routes).unwrap());
    }

    #[tokio::test]
    async fn reverse_cleanup_uses_the_device_that_created_the_mapping() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(RecordingRunner::default());
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
        *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());
        *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
            serial: "DEVICE-A".into(),
            profile_id: "profile-a".into(),
            ports: vec![31_627],
        });

        adapter.clear_active_reverse_ports().await.unwrap();

        assert_eq!(
            *runner.calls.lock().unwrap(),
            vec![vec!["-s", "DEVICE-A", "reverse", "--remove", "tcp:31627",]]
        );
        assert!(adapter.active_reverse.lock().await.is_none());
    }

    #[tokio::test]
    async fn failed_reverse_cleanup_retains_only_failed_ports_for_retry() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(SequenceRunner {
            calls: std::sync::Mutex::new(Vec::new()),
            outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
                AdbOutput {
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                },
                AdbOutput {
                    success: false,
                    stdout: String::new(),
                    stderr: "cannot remove tcp:31628".into(),
                },
                AdbOutput {
                    success: true,
                    stdout: String::new(),
                    stderr: String::new(),
                },
            ])),
        });
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
        *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
            serial: "DEVICE-A".into(),
            profile_id: "profile-a".into(),
            ports: vec![31_627, 31_628],
        });

        let error = adapter
            .clear_active_reverse_ports()
            .await
            .expect_err("部分删除失败必须可观察");
        assert_eq!(error.view_model.code, "ANDROID_ADB_COMMAND_FAILED");
        assert_eq!(
            adapter.active_reverse.lock().await.as_ref().unwrap().ports,
            vec![31_628]
        );

        adapter
            .clear_active_reverse_ports()
            .await
            .expect("重试仅删除仍归属当前运行态的端口");
        assert!(adapter.active_reverse.lock().await.is_none());
        assert_eq!(
            *runner.calls.lock().unwrap(),
            vec![
                vec!["-s", "DEVICE-A", "reverse", "--remove", "tcp:31627"],
                vec!["-s", "DEVICE-A", "reverse", "--remove", "tcp:31628"],
                vec!["-s", "DEVICE-A", "reverse", "--remove", "tcp:31628"],
            ]
        );
    }

    #[tokio::test]
    async fn device_switch_is_rejected_while_reverse_mapping_is_active() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(RecordingRunner::default());
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
        *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
            serial: "DEVICE-A".into(),
            profile_id: "profile-a".into(),
            ports: vec![31_627],
        });

        let error = adapter
            .adb_select("DEVICE-B".into())
            .await
            .expect_err("活动映射期间不能切换设备");

        assert_eq!(error.view_model.code, "ANDROID_DEVICE_SWITCH_REQUIRES_STOP");
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn destination_resolution_failure_does_not_create_reverse_mapping() {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(RecordingRunner::default());
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
        *adapter.selected_serial.write().unwrap() = Some("SER123".into());
        let activation = AndroidNetworkActivation {
            profile: AndroidNetworkProfile {
                id: "invalid-dns".into(),
                name: "invalid-dns".into(),
                target_applications: Vec::new(),
                destination_targets: Vec::new(),
                proxy_routes: Vec::new(),
                confirmed_shared_uids: std::collections::BTreeSet::default(),
                auto_resume_after_reboot: false,
                weak_network: WeakNetworkProfile::default(),
            },
            proxy_routes: vec![AndroidProxyRouteActivation {
                listener_id: "listener-invalid".into(),
                original_destination: "invalid destination".into(),
                original_ports: vec![443],
                desktop_listener_port: 8_443,
            }],
        };

        let error = adapter
            .prepare_usb_proxy_runtime(&activation)
            .await
            .expect_err("非法域名必须解析失败");

        assert_eq!(
            error.view_model.code,
            "ANDROID_PROXY_DESTINATION_RESOLVE_FAILED"
        );
        assert!(runner.calls.lock().unwrap().is_empty());
        assert!(adapter.active_reverse.lock().await.is_none());
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
