use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

pub use intercept_proxy_domain::{
    BodyCodecKind, CertificateReference, CertificateReferenceId, CertificateReferenceKind,
    ChannelId, ConnectionFaultAction, DownstreamClientAuthentication, DownstreamTlsSettings,
    FaultPreset, FaultPresetId, FixedServerSettings, ForwardProxyAuthentication,
    HttpListenerSettings, ListenerDataPlane, ListenerId, MetadataExtractor, MetadataExtractorId,
    MetadataExtractorSource, MitmSettings, ProxyListener, ProxyListenerV2, ProxyWorkspace,
    ProxyWorkspaceV2, ResponseAssertion, ResponseAssertionId, ResponseAssertionKind,
    SecretReference, SocketDownstreamTlsSettings, SocketEndpoint, SocketRelaySecurity,
    SocketRelaySettings, SocketUpstreamTlsSettings, UpstreamTlsSettings, WorkspaceId,
};

/// 标识一次代理启动周期。代理重启后旧周期的事件和断点不得继续操作。
pub type RuntimeEpoch = Uuid;
/// 应用 DTO 使用的乐观并发版本号。
pub type Revision = u64;
/// 应用层会话标识。
pub type SessionId = Uuid;
/// 应用层断点标识。
pub type BreakpointId = Uuid;
/// 应用层规则标识。
pub type RuleId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// 与具体组件库无关的视觉语义，前端只负责映射为 `HeroUI` 颜色。
pub enum UiTone {
    Neutral,
    Info,
    Positive,
    Warning,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Rust 判断某操作不可用时给出的稳定原因。
pub struct DisabledReason {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxyState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Faulted,
}

impl ProxyState {
    pub fn display_zh(self) -> (&'static str, UiTone) {
        match self {
            Self::Stopped => ("已停止", UiTone::Neutral),
            Self::Starting => ("正在启动", UiTone::Info),
            Self::Running => ("运行中", UiTone::Positive),
            Self::Stopping => ("正在停止", UiTone::Warning),
            Self::Faulted => ("故障", UiTone::Danger),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChannelState {
    Disabled,
    Stopped,
    Starting,
    Listening,
    Stopping,
    Faulted,
}

impl ChannelState {
    pub fn display_zh(self) -> (&'static str, UiTone) {
        match self {
            Self::Disabled => ("已禁用", UiTone::Neutral),
            Self::Stopped => ("已停止", UiTone::Neutral),
            Self::Starting => ("正在启动", UiTone::Info),
            Self::Listening => ("正在监听", UiTone::Positive),
            Self::Stopping => ("正在停止", UiTone::Warning),
            Self::Faulted => ("故障", UiTone::Danger),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ChannelStatusViewModel {
    pub id: ChannelId,
    pub display_name: String,
    pub state: ChannelState,
    pub state_text: String,
    pub ui_tone: UiTone,
    pub listen_address: String,
    pub mtls_enabled: bool,
    pub connected_clients: u32,
    pub request_count: u64,
    pub error_count: u64,
    pub enabled: bool,
    pub upstream_url: String,
    pub upstream_state_text: String,
    pub upstream_ui_tone: UiTone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ChannelPresentationViewModel {
    pub id: ChannelId,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionHealthState {
    Unavailable,
    Waiting,
    Healthy,
    Degraded,
    Faulted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ConnectionHealthViewModel {
    pub state: ConnectionHealthState,
    pub state_text: String,
    pub detail: String,
    pub ui_tone: UiTone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 顶部状态栏和控制台所需的完整代理状态。
/// `can_*` 与 `*_disabled_reason` 已包含业务权限判断，展示层不能自行推导。
pub struct ProxyStatusViewModel {
    pub state: ProxyState,
    pub state_text: String,
    pub ui_tone: UiTone,
    pub runtime_epoch: Option<RuntimeEpoch>,
    pub revision: Revision,
    pub channels: Vec<ChannelStatusViewModel>,
    pub app_to_proxy_health: ConnectionHealthViewModel,
    pub proxy_to_server_health: ConnectionHealthViewModel,
    pub active_sessions: usize,
    pub pending_breakpoints: usize,
    pub logical_memory_bytes: u64,
    pub logical_memory_text: String,
    pub memory_capacity_bytes: u64,
    pub memory_capacity_text: String,
    pub memory_usage_percent: u8,
    pub session_capacity: usize,
    pub default_timeout_seconds: u64,
    pub can_start: bool,
    pub start_disabled_reason: Option<DisabledReason>,
    pub can_stop: bool,
    pub stop_disabled_reason: Option<DisabledReason>,
    pub can_restart: bool,
    pub restart_disabled_reason: Option<DisabledReason>,
    pub fault_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 通用分页请求；`normalized` 会把越界页码和页大小限制到安全范围。
pub struct PageRequest {
    pub page: u32,
    pub page_size: u32,
}

impl PageRequest {
    #[must_use]
    pub fn normalized(&self) -> Self {
        Self {
            page: self.page.max(1),
            page_size: self.page_size.clamp(1, 200),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MessageStage {
    TlsHandshake,
    Request,
    Response,
    Terminal,
}

impl MessageStage {
    pub fn display_zh(self) -> &'static str {
        match self {
            Self::TlsHandshake => "TLS 握手",
            Self::Request => "请求",
            Self::Response => "响应",
            Self::Terminal => "终态",
        }
    }
}
