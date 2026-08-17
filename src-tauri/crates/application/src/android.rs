//! Android Companion control contracts shared by desktop presentation adapters.

pub use intercept_proxy_domain::{
    ANDROID_COMPANION_PACKAGE, AndroidDestinationTarget, AndroidNetworkProfile, AndroidProxyRoute,
    AndroidTargetApplication, WeakNetworkProfile,
};
use intercept_proxy_domain::{BlackoutWindow, BurstLossProfile, NthTcpFlagDrop};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use uuid::Uuid;

use crate::{AppError, AppResult, UiTone};

#[path = "android/runtime_owner.rs"]
mod runtime_owner;
pub use runtime_owner::*;

pub const ANDROID_CONTROL_PROTOCOL_VERSION: u16 = 1;
pub const ANDROID_CONTROL_MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidAdbViewModel {
    pub available: bool,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub selected_serial: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidDeviceState {
    Device,
    Offline,
    Unauthorized,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidDeviceViewModel {
    pub serial: String,
    pub state: AndroidDeviceState,
    pub product: Option<String>,
    pub model: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<String>,
    pub selected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidPackageViewModel {
    pub package_name: String,
    pub uid: u32,
    /// Set only when more than one installed package has the same Linux UID.
    pub shared_uid: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidCompanionInstallViewModel {
    pub serial: String,
    pub package_name: String,
    pub installed: bool,
    pub version_name: Option<String>,
    pub version_code: Option<String>,
}

/// Android 弱网页面的编辑意图。
/// TypeScript 只描述用户做了什么；共享 UID 扩选和嵌套故障项默认值均由
/// Rust 生成，避免展示层手写第二套领域规则。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AndroidProfileEditIntent {
    TogglePackage {
        package_name: String,
        selected: bool,
    },
    SetBurstLossEnabled {
        enabled: bool,
    },
    AddBlackoutWindow,
    AddTcpFlagDrop,
}

impl AndroidProfileEditIntent {
    pub fn apply_defaults(self, profile: &mut AndroidNetworkProfile) {
        match self {
            Self::SetBurstLossEnabled { enabled } => {
                profile.weak_network.burst_loss = enabled.then(BurstLossProfile::default);
            }
            Self::AddBlackoutWindow => {
                profile
                    .weak_network
                    .blackout_windows
                    .push(BlackoutWindow::default());
            }
            Self::AddTcpFlagDrop => {
                profile
                    .weak_network
                    .nth_tcp_flag_drops
                    .push(NthTcpFlagDrop::default());
            }
            Self::TogglePackage { .. } => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidNetworkProfileSummary {
    pub id: String,
    pub name: String,
    pub target_count: usize,
    pub auto_resume_after_reboot: bool,
}

/// Application 根据当前 Workspace 生成、交给 ADB 适配器解析 USB/LAN 链路的启动计划。
/// 它不是可持久化配置；`desktop_listener_port` 只在本次启动中使用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AndroidProxyRouteActivation {
    pub listener_id: String,
    pub original_destination: String,
    pub original_ports: Vec<u16>,
    pub desktop_listener_bind_address: String,
    pub desktop_listener_port: u16,
    pub allowed_client_cidrs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AndroidNetworkActivation {
    pub profile: AndroidNetworkProfile,
    pub proxy_routes: Vec<AndroidProxyRouteActivation>,
}

impl From<&AndroidNetworkProfile> for AndroidNetworkProfileSummary {
    fn from(profile: &AndroidNetworkProfile) -> Self {
        Self {
            id: profile.id.clone(),
            name: profile.name.clone(),
            target_count: profile.target_applications.len(),
            auto_resume_after_reboot: profile.auto_resume_after_reboot,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidControlTransport {
    LocalAbstractSocket,
    RescueActivity,
    AdbForceStop,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidNetworkState {
    Unknown,
    StartRequested,
    Running,
    StopRequested,
    Stopped,
    Faulted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidNetworkStatusViewModel {
    pub serial: String,
    pub state: AndroidNetworkState,
    /// Rust 根据状态机生成的稳定中文文案，展示层不得重复维护状态映射。
    #[serde(default)]
    pub state_text: String,
    /// Rust 生成的视觉语义；HeroUI 只负责把语义映射到组件颜色。
    #[serde(default = "default_android_status_tone")]
    pub ui_tone: UiTone,
    /// True only when the Companion protocol or a process-level emergency restore proved state.
    pub verified: bool,
    pub transport: AndroidControlTransport,
    pub active_profile_id: Option<String>,
    /// Companion 当前实际运行的 Profile 内容指纹；用于识别 Service 重启后的陈旧状态。
    #[serde(default)]
    pub active_profile_fingerprint: Option<String>,
    /// Companion 当前实际装载的透明代理路由指纹。
    #[serde(default)]
    pub active_route_fingerprint: Option<String>,
    /// Companion 当前实际装载的透明代理路由数量。
    #[serde(default)]
    pub active_route_count: usize,
    pub companion_process_running: Option<bool>,
    pub message: String,
    pub unsupported_fields: Vec<String>,
    #[specta(type = Option<specta_typescript::Unknown<Value>>)]
    pub stats: Option<Value>,
}

fn default_android_status_tone() -> UiTone {
    UiTone::Warning
}

impl AndroidNetworkStatusViewModel {
    /// 设备端协议只上报稳定状态码；最终中文展示文案由 Rust 统一生成。
    ///
    /// `state_text` 保留在公开 DTO 中，让 TypeScript 只负责渲染。这里同时兼容尚未携带
    /// 该字段的 Companion 响应，避免 Kotlin 与 Rust DTO 演进时把成功响应误判成无效
    /// JSON。
    #[must_use]
    pub fn with_rust_state_text(mut self) -> Self {
        let (state_text, ui_tone) = match self.state {
            AndroidNetworkState::Unknown => ("状态未知", UiTone::Warning),
            AndroidNetworkState::StartRequested => ("正在启动", UiTone::Info),
            AndroidNetworkState::Running if self.verified => ("运行中", UiTone::Positive),
            AndroidNetworkState::Running => ("运行中", UiTone::Warning),
            AndroidNetworkState::StopRequested => ("正在停止", UiTone::Warning),
            AndroidNetworkState::Stopped if self.verified => ("已停止", UiTone::Neutral),
            AndroidNetworkState::Stopped => ("已停止", UiTone::Warning),
            AndroidNetworkState::Faulted => ("故障", UiTone::Danger),
        };
        state_text.clone_into(&mut self.state_text);
        self.ui_tone = ui_tone;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AndroidControlRequest {
    pub version: u16,
    pub request_id: Uuid,
    pub operation: String,
    pub payload: Value,
}

impl AndroidControlRequest {
    pub fn new(operation: impl Into<String>, payload: Value) -> AppResult<Self> {
        let operation = operation.into();
        if !matches!(
            operation.as_str(),
            "profile_list"
                | "profile_get"
                | "profile_save"
                | "profile_delete"
                | "start"
                | "apply"
                | "stop"
                | "emergency_restore"
                | "status"
        ) {
            return Err(AppError::new(
                "ANDROID_PROTOCOL_OPERATION_INVALID",
                "Android 控制操作不在协议白名单中。",
            ));
        }
        let request = Self {
            version: ANDROID_CONTROL_PROTOCOL_VERSION,
            request_id: Uuid::new_v4(),
            operation,
            payload,
        };
        let encoded = serde_json::to_vec(&request)
            .map_err(|error| AppError::new("ANDROID_PROTOCOL_ENCODE_FAILED", error.to_string()))?;
        if encoded.len() > ANDROID_CONTROL_MAX_FRAME_BYTES {
            return Err(AppError::new(
                "ANDROID_PROTOCOL_FRAME_TOO_LARGE",
                "Android 控制请求超过 1 MiB 上限。",
            ));
        }
        Ok(request)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AndroidControlResponse {
    pub version: u16,
    pub request_id: Uuid,
    pub ok: bool,
    pub status: Option<AndroidNetworkStatusViewModel>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

pub fn encode_android_control_frame<T: Serialize>(value: &T) -> AppResult<Vec<u8>> {
    let payload = serde_json::to_vec(value)
        .map_err(|error| AppError::new("ANDROID_PROTOCOL_ENCODE_FAILED", error.to_string()))?;
    let length = u32::try_from(payload.len()).map_err(|_| {
        AppError::new(
            "ANDROID_PROTOCOL_FRAME_TOO_LARGE",
            "Android 控制帧长度溢出。",
        )
    })?;
    if payload.len() > ANDROID_CONTROL_MAX_FRAME_BYTES {
        return Err(AppError::new(
            "ANDROID_PROTOCOL_FRAME_TOO_LARGE",
            "Android 控制帧超过 1 MiB 上限。",
        ));
    }
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_android_control_frame<T: for<'de> Deserialize<'de>>(frame: &[u8]) -> AppResult<T> {
    if frame.len() < 4 {
        return Err(AppError::new(
            "ANDROID_PROTOCOL_FRAME_INVALID",
            "Android 控制帧缺少长度前缀。",
        ));
    }
    let declared = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    if declared > ANDROID_CONTROL_MAX_FRAME_BYTES || declared != frame.len() - 4 {
        return Err(AppError::new(
            "ANDROID_PROTOCOL_FRAME_INVALID",
            "Android 控制帧长度无效。",
        ));
    }
    serde_json::from_slice(&frame[4..])
        .map_err(|error| AppError::new("ANDROID_PROTOCOL_JSON_INVALID", error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::*;

    #[test]
    fn length_prefixed_protocol_rejects_truncation_and_oversize() {
        let request = AndroidControlRequest::new("status", json!({})).unwrap();
        let frame = encode_android_control_frame(&request).unwrap();
        assert_eq!(
            decode_android_control_frame::<AndroidControlRequest>(&frame).unwrap(),
            request
        );
        assert!(
            decode_android_control_frame::<AndroidControlRequest>(&frame[..frame.len() - 1])
                .is_err()
        );
        let mut oversized = vec![0; 4];
        oversized.copy_from_slice(
            &u32::try_from(ANDROID_CONTROL_MAX_FRAME_BYTES + 1)
                .unwrap()
                .to_be_bytes(),
        );
        assert!(decode_android_control_frame::<AndroidControlRequest>(&oversized).is_err());
    }

    #[test]
    fn profile_rejects_companion_and_requires_confirmation_for_total_loss() {
        let profile = AndroidNetworkProfile {
            id: "danger".into(),
            name: "Danger".into(),
            target_applications: vec![AndroidTargetApplication {
                package_name: ANDROID_COMPANION_PACKAGE.into(),
                uid: 10_000,
                display_name: None,
            }],
            destination_targets: vec![AndroidDestinationTarget {
                cidr: "10.0.34.0/24".into(),
                ports: vec![443, 16_127],
            }],
            proxy_routes: Vec::new(),
            confirmed_shared_uids: BTreeSet::new(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile {
                random_loss_basis_points: 10_000,
                ..WeakNetworkProfile::default()
            },
        };
        assert!(profile.validate().is_err());
        assert!(profile.requires_dangerous_confirmation());
    }

    #[test]
    fn profile_accepts_multiple_destination_addresses_and_rejects_invalid_ranges() {
        let mut profile = AndroidNetworkProfile {
            id: "multiple-addresses".into(),
            name: "Multiple addresses".into(),
            target_applications: vec![AndroidTargetApplication {
                package_name: "com.example.client".into(),
                uid: 10_001,
                display_name: None,
            }],
            destination_targets: vec![
                AndroidDestinationTarget {
                    cidr: "10.0.34.50".into(),
                    ports: vec![16_127, 16_627],
                },
                AndroidDestinationTarget {
                    cidr: "2001:db8::/32".into(),
                    ports: Vec::new(),
                },
            ],
            proxy_routes: Vec::new(),
            confirmed_shared_uids: BTreeSet::new(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        };
        profile.validate().expect("多个地址应通过校验");

        profile.destination_targets[1].cidr = "2001:db8::/129".into();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn companion_wire_status_without_display_text_is_normalized_by_rust() {
        let status: AndroidNetworkStatusViewModel = serde_json::from_value(json!({
            "serial": "",
            "state": "running",
            "verified": true,
            "transport": "local_abstract_socket",
            "active_profile_id": "profile-1",
            "companion_process_running": true,
            "message": "native running",
            "unsupported_fields": ["serial"],
            "stats": null
        }))
        .expect("Companion wire response may omit display-only state_text");

        assert!(status.state_text.is_empty());
        let normalized = status.with_rust_state_text();
        assert_eq!(normalized.state_text, "运行中");
        assert_eq!(normalized.ui_tone, UiTone::Positive);
    }

    #[test]
    fn nested_edit_defaults_are_owned_by_rust() {
        let mut profile = AndroidNetworkProfile {
            id: "defaults".into(),
            name: "Defaults".into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            proxy_routes: Vec::new(),
            confirmed_shared_uids: BTreeSet::new(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        };

        AndroidProfileEditIntent::SetBurstLossEnabled { enabled: true }
            .apply_defaults(&mut profile);
        AndroidProfileEditIntent::AddBlackoutWindow.apply_defaults(&mut profile);
        AndroidProfileEditIntent::AddTcpFlagDrop.apply_defaults(&mut profile);

        assert_eq!(
            profile.weak_network.burst_loss,
            Some(BurstLossProfile::default())
        );
        assert_eq!(
            profile.weak_network.blackout_windows,
            vec![BlackoutWindow::default()]
        );
        assert_eq!(
            profile.weak_network.nth_tcp_flag_drops,
            vec![NthTcpFlagDrop::default()]
        );
    }
}
