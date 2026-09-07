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

use crate::UiTone;

pub use intercept_proxy_android_control_protocol::{
    ANDROID_CONTROL_MAX_FRAME_BYTES, ANDROID_CONTROL_PROTOCOL_VERSION, AndroidCompanionStatus,
    AndroidControlRequest, AndroidControlResponse, AndroidControlTransport, AndroidNetworkState,
    decode_android_control_frame, encode_android_control_frame,
};

#[path = "android/protocol_error.rs"]
mod protocol_error;

#[path = "android/runtime_owner.rs"]
mod runtime_owner;
pub use runtime_owner::*;
#[path = "android/runtime_endpoint.rs"]
mod runtime_endpoint;
pub use runtime_endpoint::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct AndroidDeviceTarget {
    pub serial: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct AndroidRuntimeTarget {
    pub serial: String,
    pub expected_epoch: Uuid,
}

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
    pub stop_vpn_on_control_loss: bool,
}

/// Application 根据当前 Workspace 生成、交给 ADB 适配器解析 USB/LAN 链路的启动计划。
/// 它不是可持久化配置；`desktop_listener_port` 只在本次启动中使用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AndroidProxyRouteActivation {
    pub listener_id: String,
    pub listener_name: String,
    pub original_destination: String,
    pub original_ports: Vec<u16>,
    pub desktop_listener_bind_address: String,
    pub desktop_listener_port: u16,
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
            stop_vpn_on_control_loss: profile.stop_vpn_on_control_loss,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidNetworkStatusViewModel {
    pub serial: String,
    pub runtime_epoch: Option<Uuid>,
    pub state: AndroidNetworkState,
    /// Rust 根据状态机生成的稳定中文文案，展示层不得重复维护状态映射。
    pub state_text: String,
    /// Rust 生成的视觉语义；HeroUI 只负责把语义映射到组件颜色。
    pub ui_tone: UiTone,
    /// True only when the Companion protocol or a process-level emergency restore proved state.
    pub verified: bool,
    pub transport: AndroidControlTransport,
    pub active_profile_id: Option<String>,
    /// Companion 当前实际运行的 Profile 内容指纹；用于识别 Service 重启后的陈旧状态。
    pub active_profile_fingerprint: Option<String>,
    /// Companion 当前实际装载的透明代理路由指纹。
    pub active_route_fingerprint: Option<String>,
    /// Companion 当前实际装载的透明代理路由数量。
    pub active_route_count: usize,
    pub companion_process_running: Option<bool>,
    pub message: String,
    pub unsupported_fields: Vec<String>,
    #[specta(type = Option<specta_typescript::Unknown<Value>>)]
    pub stats: Option<Value>,
}

impl AndroidNetworkStatusViewModel {
    /// 设备端协议只上报稳定状态码；最终中文展示文案由 Rust 统一生成。
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

impl From<AndroidCompanionStatus> for AndroidNetworkStatusViewModel {
    fn from(status: AndroidCompanionStatus) -> Self {
        AndroidNetworkStatusViewModel {
            serial: status.serial,
            runtime_epoch: None,
            state: status.state,
            state_text: String::new(),
            ui_tone: UiTone::Neutral,
            verified: status.verified,
            transport: status.transport,
            active_profile_id: status.active_profile_id,
            active_profile_fingerprint: status.active_profile_fingerprint,
            active_route_fingerprint: status.active_route_fingerprint,
            active_route_count: status.active_route_count,
            companion_process_running: status.companion_process_running,
            message: status.message,
            unsupported_fields: status.unsupported_fields,
            stats: status.stats,
        }
        .with_rust_state_text()
    }
}

#[cfg(test)]
#[path = "android/tests.rs"]
mod tests;
