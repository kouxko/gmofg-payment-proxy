//! 与 Android Companion 的版本化控制协议。
//!
//! ADB 只负责把本地临时端口转发到设备的 localabstract socket；请求 ID、协议版本、
//! 帧上限和最终运行状态都在这里校验，避免把 Activity 启动成功误判为 VPN 已运行。

use std::{
    collections::BTreeSet,
    fmt::Write as _,
    net::TcpListener as StdTcpListener,
    sync::{Mutex as StdMutex, OnceLock},
    time::Duration,
};

use intercept_proxy_application::{
    ANDROID_COMPANION_PACKAGE, ANDROID_CONTROL_MAX_FRAME_BYTES, ANDROID_CONTROL_PROTOCOL_VERSION,
    AndroidControlRequest, AndroidControlResponse, AndroidControlTransport, AndroidNetworkState,
    AndroidNetworkStatusViewModel, AppError, AppResult, UiTone, encode_android_control_frame,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use super::{ActiveRuntimeFacts, AndroidAdbAdapter, COMMAND_TIMEOUT, CONTROL_SOCKET};
use crate::adapters::android_adb::command::AdbOutput;
use crate::adapters::android_adb::command::is_missing_adb_listener_error;

const ACTIVATION_STATUS_ATTEMPTS: usize = 20;
const ACTIVATION_STATUS_INTERVAL: Duration = Duration::from_millis(250);

impl AndroidAdbAdapter {
    /// 使用 Android 系统进程管理器强制关闭 Companion。
    ///
    /// 这是优雅 stop 控制协议失效时的安全兜底：进程退出会关闭 TUN 文件描述符，
    /// 因而目标应用恢复系统网络。ADB reverse 的桌面端所有权由调用者随后统一清理。
    pub(super) async fn force_stop_companion(
        &self,
        serial: &str,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        self.run_for_serial(
            serial,
            &["shell", "am", "force-stop", ANDROID_COMPANION_PACKAGE],
            COMMAND_TIMEOUT,
        )
        .await?;
        Ok(AndroidNetworkStatusViewModel {
            serial: serial.to_owned(),
            runtime_epoch: None,
            state: AndroidNetworkState::Stopped,
            state_text: "已停止".into(),
            ui_tone: UiTone::Neutral,
            verified: true,
            transport: AndroidControlTransport::AdbForceStop,
            active_profile_id: None,
            active_profile_fingerprint: None,
            active_route_fingerprint: None,
            active_route_count: 0,
            companion_process_running: Some(false),
            message: "控制协议不可用，已通过 ADB 强制停止设备端组件并关闭 TUN。".into(),
            unsupported_fields: vec!["last_profile_id".into(), "packet_stats".into()],
            stats: None,
        })
    }

    pub(super) async fn protocol_request(
        &self,
        serial: &str,
        operation: &str,
        payload: Value,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let request = AndroidControlRequest::new(operation, payload)?;
        let port_reservation = reserve_loopback_port()?;
        let port = port_reservation.port;
        self.run_forward_for_serial(
            serial,
            &[
                "forward",
                &format!("tcp:{port}"),
                &format!("localabstract:{CONTROL_SOCKET}"),
            ],
        )
        .await?;
        let response_serial = serial.to_owned();
        let result = self.exchange_frame(port, request).await.map(|mut status| {
            // ADB serial 属于桌面选择上下文，只能在协议校验完成后补回。
            status.serial = response_serial;
            status
        });
        let cleanup = self
            .run_forward_for_serial(serial, &["forward", "--remove", &format!("tcp:{port}")])
            .await;
        drop(port_reservation);
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
            .map(intercept_proxy_application::AndroidCompanionStatus::into_view_model)
            .ok_or_else(|| {
                AppError::new(
                    "ANDROID_PROTOCOL_RESPONSE_INVALID",
                    "设备端成功响应缺少状态。",
                )
            })
    }

    /// Activity 只负责唤醒 Companion 进程；最终成功仍由版本化控制协议证明。
    pub(super) async fn wake_control_server(&self, serial: &str) -> AppResult<()> {
        self.run_for_serial(
            serial,
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

    pub(super) async fn protocol_request_after_wake(
        &self,
        serial: &str,
        operation: &str,
        payload: Value,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let _wake_result = self.wake_control_server(serial).await;
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(150 * attempt)).await;
            }
            let result = self
                .protocol_request(serial, operation, payload.clone())
                .await;
            if !(attempt < 2 && result.as_ref().is_err_and(is_socket_unavailable)) {
                return result;
            }
        }
        unreachable!("bounded control retry always returns on its final attempt")
    }

    /// `start`/`apply` 只表示设备已接受异步请求；必须继续观察到匹配本次指纹的
    /// `Running` 状态后，桌面端才可以提交暂存的 ADB reverse 映射。
    pub(super) async fn confirm_network_running(
        &self,
        runtime: &ActiveRuntimeFacts,
        initial: AndroidNetworkStatusViewModel,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        match classify_activation_status(runtime, &initial) {
            ActivationObservation::Confirmed => return Ok(initial),
            ActivationObservation::Faulted => return Err(activation_failed_error(&initial)),
            ActivationObservation::Pending => {}
        }
        for attempt in 0..ACTIVATION_STATUS_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(ACTIVATION_STATUS_INTERVAL).await;
            }
            let status = self
                .protocol_request(&runtime.serial, "status", json!({}))
                .await?;
            match classify_activation_status(runtime, &status) {
                ActivationObservation::Confirmed => return Ok(status),
                ActivationObservation::Faulted => return Err(activation_failed_error(&status)),
                ActivationObservation::Pending => {}
            }
        }
        Err(AppError::new(
            "ANDROID_NETWORK_START_CONFIRMATION_TIMEOUT",
            "设备已接受网络接管请求，但未在 5 秒内确认 VPN 与透明代理路由进入运行状态。",
        )
        .retryable("请刷新设备运行状态；若设备报告故障，请停止后重新启动网络接管。"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActivationObservation {
    Confirmed,
    Pending,
    Faulted,
}

pub(super) fn classify_activation_status(
    runtime: &ActiveRuntimeFacts,
    status: &AndroidNetworkStatusViewModel,
) -> ActivationObservation {
    if status.state == AndroidNetworkState::Faulted {
        return ActivationObservation::Faulted;
    }
    if status.state == AndroidNetworkState::Running
        && status.verified
        && status.active_profile_id.as_deref() == Some(runtime.profile_id.as_str())
        && status.active_profile_fingerprint.as_deref()
            == Some(runtime.profile_fingerprint.as_str())
        && status.active_route_fingerprint.as_deref() == Some(runtime.route_fingerprint.as_str())
        && status.active_route_count == runtime.route_count
    {
        ActivationObservation::Confirmed
    } else {
        ActivationObservation::Pending
    }
}

fn activation_failed_error(status: &AndroidNetworkStatusViewModel) -> AppError {
    AppError::new(
        "ANDROID_NETWORK_START_FAILED",
        format!("设备未能启动网络接管：{}", status.message),
    )
    .retryable("请检查 Android 前台通知和设备运行状态，修正后重新启动网络接管。")
}

pub(super) struct ControlPortReservation {
    port: u16,
}

#[cfg(test)]
impl ControlPortReservation {
    pub(super) fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for ControlPortReservation {
    fn drop(&mut self) {
        control_port_reservations()
            .lock()
            .expect("control port reservations")
            .remove(&self.port);
    }
}

fn control_port_reservations() -> &'static StdMutex<BTreeSet<u16>> {
    static RESERVATIONS: OnceLock<StdMutex<BTreeSet<u16>>> = OnceLock::new();
    RESERVATIONS.get_or_init(|| StdMutex::new(BTreeSet::new()))
}

pub(super) fn reserve_loopback_port() -> AppResult<ControlPortReservation> {
    for _ in 0..32 {
        let listener = StdTcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
            AppError::new(
                "ANDROID_ADB_FORWARD_INVALID",
                format!("无法分配 Android 控制通道本地端口：{error}"),
            )
        })?;
        let port = listener
            .local_addr()
            .map(|address| address.port())
            .map_err(|error| {
                AppError::new(
                    "ANDROID_ADB_FORWARD_INVALID",
                    format!("无法读取 Android 控制通道本地端口：{error}"),
                )
            })?;
        let mut reservations = control_port_reservations()
            .lock()
            .expect("control port reservations");
        if reservations.insert(port) {
            return Ok(ControlPortReservation { port });
        }
    }
    Err(AppError::new(
        "ANDROID_ADB_FORWARD_INVALID",
        "无法分配未被并发 Android 控制请求占用的本地端口。",
    ))
}

pub(super) fn is_socket_unavailable(error: &AppError) -> bool {
    matches!(
        error.view_model.code.as_str(),
        "ANDROID_CONTROL_SOCKET_UNAVAILABLE"
            | "ANDROID_CONTROL_SOCKET_TIMEOUT"
            | "ANDROID_CONTROL_SOCKET_FAILED"
    )
}

pub(super) fn fallback_unsupported_fields() -> Vec<String> {
    vec![
        "vpn_running".into(),
        "active_profile_id".into(),
        "packet_stats".into(),
        "data_plane_available".into(),
    ]
}

pub(super) fn reconcile_forward_cleanup<T>(
    result: AppResult<T>,
    cleanup: AppResult<AdbOutput>,
) -> AppResult<T> {
    match (result, cleanup) {
        (Ok(value), Ok(_)) => Ok(value),
        // ADB 可能已随设备断开自动移除临时 forward。这个特定结果等价于清理完成，
        // 不能反转已经成功的 stop/status 响应。
        (Ok(value), Err(cleanup_error)) if is_missing_adb_listener_error(&cleanup_error) => {
            tracing::warn!(
                code = cleanup_error.view_model.code,
                message = cleanup_error.view_model.message,
                "adb forward was already absent after a successful control request"
            );
            Ok(value)
        }
        // 其他清理失败可能真的遗留端口，必须显式暴露，不能静默吞掉。
        (Ok(_), Err(cleanup_error)) => Err(AppError::new(
            "ANDROID_ADB_FORWARD_CLEANUP_FAILED",
            format!(
                "设备控制请求已完成，但临时 ADB forward 清理失败：{}",
                cleanup_error.view_model.message
            ),
        )
        .retryable("请刷新设备状态后重试；必要时执行紧急恢复网络。")),
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(cleanup_error)) if is_missing_adb_listener_error(&cleanup_error) => {
            Err(error)
        }
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
