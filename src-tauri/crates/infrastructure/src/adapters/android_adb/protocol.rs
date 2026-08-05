//! 与 Android Companion 的版本化控制协议。
//!
//! ADB 只负责把本地临时端口转发到设备的 localabstract socket；请求 ID、协议版本、
//! 帧上限和最终运行状态都在这里校验，避免把 Activity 启动成功误判为 VPN 已运行。

use std::{fmt::Write as _, net::TcpListener as StdTcpListener, time::Duration};

use intercept_proxy_application::{
    ANDROID_CONTROL_MAX_FRAME_BYTES, ANDROID_CONTROL_PROTOCOL_VERSION, AndroidControlRequest,
    AndroidControlResponse, AndroidNetworkState, AndroidNetworkStatusViewModel, AppError,
    AppResult, encode_android_control_frame,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use super::{ActiveRuntimeFacts, AndroidAdbAdapter, COMMAND_TIMEOUT, CONTROL_SOCKET};
use crate::adapters::android_adb::command::AdbOutput;

const ACTIVATION_STATUS_ATTEMPTS: usize = 20;
const ACTIVATION_STATUS_INTERVAL: Duration = Duration::from_millis(250);

impl AndroidAdbAdapter {
    pub(super) async fn protocol_request(
        &self,
        operation: &str,
        payload: Value,
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
            // ADB serial 属于桌面选择上下文，只能在协议校验完成后补回。
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

    /// Activity 只负责唤醒 Companion 进程；最终成功仍由版本化控制协议证明。
    pub(super) async fn wake_control_server(&self) -> AppResult<()> {
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

    pub(super) async fn protocol_request_after_wake(
        &self,
        operation: &str,
        payload: Value,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
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
            let status = self.protocol_request("status", json!({})).await?;
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
