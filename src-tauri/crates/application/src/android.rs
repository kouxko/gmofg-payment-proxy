//! Android Companion control contracts shared by desktop presentation adapters.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use intercept_proxy_domain::{
    BlackoutWindow, BurstLossProfile, NthTcpFlagDrop, WeakNetworkProfile,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use uuid::Uuid;

use crate::{AppError, AppResult, UiTone};

pub const ANDROID_CONTROL_PROTOCOL_VERSION: u16 = 1;
pub const ANDROID_CONTROL_MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const ANDROID_COMPANION_PACKAGE: &str = "com.interceptproxy.vpn";

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
    pub signing_sha256: Option<String>,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidTargetApplication {
    pub package_name: String,
    pub signing_sha256: String,
    pub uid: u32,
    pub display_name: Option<String>,
}

/// Android 弱网 Profile 需要处理的一段远端地址范围。
/// 一个 Profile 可以保存任意多个目标；`cidr` 同时接受单个 IPv4/IPv6 地址和
/// CIDR（例如 `10.0.34.20`、`10.0.34.0/24`、`2001:db8::/32`）。`ports`
/// 为空表示该地址范围的全部端口。这里刻意不接受域名：TUN 数据面只能可靠观察
/// IP 包，不能把 DNS 名称伪装成每条连接都稳定存在的属性。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidDestinationTarget {
    pub cidr: String,
    pub ports: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidNetworkProfile {
    pub id: String,
    pub name: String,
    pub target_applications: Vec<AndroidTargetApplication>,
    /// 需要实施弱网的远端地址列表。空列表表示保留原行为：目标应用访问的全部
    /// 原始地址都进入弱网引擎，因此不会把一个应用错误限制为单一 Server。
    #[serde(default)]
    pub destination_targets: Vec<AndroidDestinationTarget>,
    pub confirmed_shared_uids: BTreeSet<u32>,
    pub auto_resume_after_reboot: bool,
    /// 弱网字段由 Rust 领域模型统一定义，并生成前端只读 TypeScript 类型。
    pub weak_network: WeakNetworkProfile,
}

/// Android 弱网页面的编辑意图。
/// TypeScript 只描述用户做了什么；共享 UID 扩选、签名快照和嵌套故障项默认值均由
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

impl AndroidNetworkProfile {
    pub fn validate(&self) -> AppResult<()> {
        let mut fields = BTreeMap::<String, Vec<String>>::new();
        if self.id.is_empty() || self.id.len() > 128 || !is_safe_profile_id(&self.id) {
            fields.insert(
                "id".into(),
                vec![
                    "弱网方案 ID 只能包含字母、数字、点、下划线和连字符，且不超过 128 字节。"
                        .into(),
                ],
            );
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            fields.insert(
                "name".into(),
                vec!["弱网方案名称不能为空且不能超过 80 个字符。".into()],
            );
        }
        if self.target_applications.is_empty() || self.target_applications.len() > 64 {
            fields.insert(
                "target_applications".into(),
                vec!["必须选择 1 到 64 个目标应用。".into()],
            );
        }
        let mut packages = BTreeSet::new();
        for (index, target) in self.target_applications.iter().enumerate() {
            let prefix = format!("target_applications.{index}");
            if target.package_name == ANDROID_COMPANION_PACKAGE {
                fields
                    .entry(prefix.clone())
                    .or_default()
                    .push("设备端组件自身不能进入网络接管允许列表。".into());
            }
            if !is_android_package_name(&target.package_name)
                || !packages.insert(&target.package_name)
            {
                fields
                    .entry(format!("{prefix}.package_name"))
                    .or_default()
                    .push("包名无效或重复。".into());
            }
            if target.uid == 0 {
                fields
                    .entry(format!("{prefix}.uid"))
                    .or_default()
                    .push("UID 必须大于 0。".into());
            }
            if !is_sha256_set(&target.signing_sha256) {
                fields
                    .entry(format!("{prefix}.signing_sha256"))
                    .or_default()
                    .push("签名必须是一个或多个以 + 连接的 SHA-256 指纹。".into());
            }
        }
        self.validate_destination_targets(&mut fields);
        let weak_bytes = serde_json::to_vec(&self.weak_network).map_err(|error| {
            AppError::new(
                "ANDROID_PROFILE_INVALID",
                format!("弱网配置无法序列化：{error}"),
            )
        })?;
        if weak_bytes.len() > 256 * 1024 {
            fields.insert(
                "weak_network".into(),
                vec!["弱网配置必须是对象且不能超过 256 KiB。".into()],
            );
        }
        if fields.is_empty() {
            Ok(())
        } else {
            Err(AppError::field(
                "ANDROID_PROFILE_INVALID",
                "弱网方案校验失败。",
                fields,
            ))
        }
    }

    #[must_use]
    pub fn requires_dangerous_confirmation(&self) -> bool {
        self.weak_network.random_loss_basis_points >= 10_000
            || !self.weak_network.blackout_windows.is_empty()
    }

    fn validate_destination_targets(&self, fields: &mut BTreeMap<String, Vec<String>>) {
        if self.destination_targets.len() > 128 {
            fields.insert(
                "destination_targets".into(),
                vec!["一个 Profile 最多配置 128 个目标地址范围。".into()],
            );
        }
        let mut destination_targets = BTreeSet::new();
        for (index, target) in self.destination_targets.iter().enumerate() {
            let prefix = format!("destination_targets.{index}");
            if !is_valid_ip_or_cidr(&target.cidr) {
                fields
                    .entry(format!("{prefix}.cidr"))
                    .or_default()
                    .push("请输入单个 IP 或合法 IPv4/IPv6 CIDR。".into());
            }
            let normalized = target.cidr.trim().to_ascii_lowercase();
            if !destination_targets.insert((normalized, target.ports.clone())) {
                fields
                    .entry(prefix.clone())
                    .or_default()
                    .push("目标地址范围与端口组合不能重复。".into());
            }
            let mut ports = BTreeSet::new();
            if target
                .ports
                .iter()
                .any(|port| *port == 0 || !ports.insert(*port))
            {
                fields
                    .entry(format!("{prefix}.ports"))
                    .or_default()
                    .push("端口必须位于 1..=65535 且不能重复。".into());
            }
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
    let declared = u32::from_be_bytes(frame[..4].try_into().expect("four-byte prefix")) as usize;
    if declared > ANDROID_CONTROL_MAX_FRAME_BYTES || declared != frame.len() - 4 {
        return Err(AppError::new(
            "ANDROID_PROTOCOL_FRAME_INVALID",
            "Android 控制帧长度无效。",
        ));
    }
    serde_json::from_slice(&frame[4..])
        .map_err(|error| AppError::new("ANDROID_PROTOCOL_JSON_INVALID", error.to_string()))
}

fn is_safe_profile_id(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_android_package_name(value: &str) -> bool {
    value.len() <= 255
        && value.contains('.')
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

fn is_sha256_set(value: &str) -> bool {
    !value.is_empty()
        && value.split('+').all(|digest| {
            let compact = digest.replace(':', "");
            compact.len() == 64 && compact.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn is_valid_ip_or_cidr(value: &str) -> bool {
    let value = value.trim();
    let Some((address, prefix)) = value.split_once('/') else {
        return value.parse::<IpAddr>().is_ok();
    };
    let Ok(address) = address.parse::<IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    match address {
        IpAddr::V4(_) => prefix <= 32,
        IpAddr::V6(_) => prefix <= 128,
    }
}

#[cfg(test)]
mod tests {
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
                signing_sha256: "AA:".repeat(31) + "AA",
                uid: 10_000,
                display_name: None,
            }],
            destination_targets: vec![AndroidDestinationTarget {
                cidr: "10.0.34.0/24".into(),
                ports: vec![443, 16_127],
            }],
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
                signing_sha256: "AA".repeat(32),
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
