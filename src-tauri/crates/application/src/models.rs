//! 应用层输入模型、ViewModel 与实时事件 DTO。
//!
//! 输入模型表达“用户想做什么”，ViewModel 表达“界面应显示什么”。筛选、分页、中文
//! 状态文案和可操作性都由 Rust 计算，TypeScript 只负责渲染。

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use uuid::Uuid;

pub use intercept_proxy_domain::{
    BodyCodecKind, CertificateReference, CertificateReferenceId, CertificateReferenceKind,
    ChannelId, ConnectionFaultAction, FaultPreset, FaultPresetId, ListenerId, MetadataExtractor,
    MetadataExtractorId, MetadataExtractorSource, ProxyListener, ProxyWorkspace, ResponseAssertion,
    ResponseAssertionId, ResponseAssertionKind, SecretReference, WorkspaceId,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 抓包页提交给 Rust 的筛选、增量游标、排序与分页条件。
pub struct CaptureQuery {
    pub keyword: Option<String>,
    pub terminal_ip: Option<String>,
    pub channel: Option<ChannelId>,
    pub stage: Option<MessageStage>,
    pub result: Option<String>,
    pub rule_id: Option<RuleId>,
    /// 设置后只返回内存中仍保留、且比该游标新的记录。
    pub after_event_id: Option<u64>,
    pub sort: CaptureSort,
    pub direction: SortDirection,
    pub page: PageRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSort {
    OccurredAt,
    TerminalIp,
    Duration,
    Size,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 抓包表格中的轻量行，不包含完整 Payload。
pub struct CaptureRowViewModel {
    pub event_id: u64,
    pub runtime_epoch: RuntimeEpoch,
    pub session_id: SessionId,
    pub occurred_at: DateTime<Utc>,
    pub terminal_ip: String,
    pub channel: ChannelId,
    pub channel_text: String,
    pub stage: MessageStage,
    pub stage_text: String,
    pub method: String,
    pub target: String,
    /// 生成此行时已知的 HTTP 响应码。请求阶段通常为空；响应/终态行可直接显示，
    /// 无需界面先加载完整 Payload。
    pub http_status: Option<u16>,
    pub result: String,
    pub ui_tone: UiTone,
    pub duration_ms: Option<u64>,
    pub matched_rule_ids: Vec<RuleId>,
    pub size_bytes: u64,
    pub breakpoint_id: Option<BreakpointId>,
    pub can_go_to_breakpoint: bool,
    pub breakpoint_disabled_reason: Option<DisabledReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CapturePageViewModel {
    pub rows: Vec<CaptureRowViewModel>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
    pub event_cursor: u64,
    pub oldest_event_id: Option<u64>,
    pub runtime_epoch: Option<RuntimeEpoch>,
    pub snapshot_required: bool,
    pub empty_message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 为无损 HTTP/1 往返保留的一条原始 Header。
/// Header 可能重复、大小写不同或包含有意义的空白，因此不能只用 Map 保存。
pub struct RawHttpHeaderViewModel {
    /// 字段名的精确线上字节，是断点转发时的权威表示；普通 `headers` 只是有损展示投影。
    pub name_bytes: Vec<u8>,
    /// 字段值的精确字节，不含可选空白和 CRLF。
    pub value_bytes: Vec<u8>,
    /// 冒号与实际字段值之间的原始可选空白。
    #[serde(default)]
    pub leading_ows_bytes: Vec<u8>,
    /// 实际字段值之后的原始可选空白。
    #[serde(default)]
    pub trailing_ows_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 可供界面查看/编辑，同时可无损重建网络报文的内容模型。
pub struct MessageContentViewModel {
    pub http_status: Option<u16>,
    /// 精确起始行字节，避免把展示字符串误作报文重建来源。
    #[serde(default)]
    pub start_line_bytes: Vec<u8>,
    /// 保留名称、值、大小写、重复项和原始顺序的 Header。
    #[serde(default)]
    pub raw_headers: Vec<RawHttpHeaderViewModel>,
    /// 仅供展示和表单编辑的有损分组投影。
    pub headers: BTreeMap<String, Vec<String>>,
    pub body_text: Option<String>,
    pub body_bytes: Vec<u8>,
    #[specta(type = Option<specta_typescript::Unknown<Value>>)]
    pub json: Option<Value>,
    pub content_length: usize,
}

impl MessageContentViewModel {
    pub const ENTITY_FIXED_OVERHEAD_BYTES: u64 = 128;

    pub fn logical_bytes(&self) -> u64 {
        let headers = self
            .headers
            .iter()
            .map(|(name, values)| name.len() + values.iter().map(String::len).sum::<usize>())
            .sum::<usize>();
        let raw_headers = self
            .raw_headers
            .iter()
            .map(|header| {
                header.name_bytes.len()
                    + header.leading_ows_bytes.len()
                    + header.value_bytes.len()
                    + header.trailing_ows_bytes.len()
            })
            .sum::<usize>();
        Self::ENTITY_FIXED_OVERHEAD_BYTES
            + (self.start_line_bytes.len() + headers + raw_headers + self.body_bytes.len()) as u64
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct CaptureDetailViewModel {
    pub session_id: SessionId,
    pub request_id: String,
    pub terminal_ip: String,
    pub certificate_fingerprint_suffix: String,
    pub upstream_host: String,
    pub request: MessageContentViewModel,
    pub response: Option<MessageContentViewModel>,
    pub tls_summary: String,
    pub timings_ms: BTreeMap<String, u64>,
    pub rule_trace: Vec<String>,
    pub extracted_metadata: BTreeMap<String, String>,
    pub response_assertions: Vec<ResponseAssertionResultViewModel>,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 会话页提交给 Rust 的筛选、时间范围、排序和分页条件。
pub struct SessionQuery {
    pub keyword: Option<String>,
    pub terminal_ip: Option<String>,
    pub channel: Option<ChannelId>,
    pub result: Option<String>,
    pub rule_id: Option<RuleId>,
    pub started_from: Option<DateTime<Utc>>,
    pub started_to: Option<DateTime<Utc>>,
    pub sort: SessionSort,
    pub direction: SortDirection,
    pub page: PageRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionSort {
    StartedAt,
    TerminalIp,
    Duration,
    RequestSize,
    ResponseSize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionSummaryViewModel {
    pub session_id: SessionId,
    pub request_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub terminal_ip: String,
    pub channel: ChannelId,
    pub channel_text: String,
    pub method: String,
    pub target: String,
    pub http_status: Option<u16>,
    pub result: String,
    pub ui_tone: UiTone,
    pub duration_ms: Option<u64>,
    pub matched_rule_ids: Vec<RuleId>,
    pub request_size_bytes: u64,
    pub response_size_bytes: u64,
    pub pending_breakpoint: bool,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionPageViewModel {
    pub items: Vec<SessionSummaryViewModel>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
    pub empty_message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 会话详情；完整请求/响应只在用户打开详情时返回。
pub struct SessionDetailViewModel {
    pub summary: SessionSummaryViewModel,
    pub runtime_epoch: RuntimeEpoch,
    pub connection_id: String,
    pub certificate_fingerprint: String,
    pub upstream_host: String,
    pub app_to_proxy_tls: String,
    pub proxy_to_server_tls: String,
    pub final_action: String,
    pub timings_ms: BTreeMap<String, u64>,
    pub request: Option<MessageContentViewModel>,
    pub response: Option<MessageContentViewModel>,
    pub rule_trace: Vec<String>,
    /// Workspace 元数据提取器生成的少量文本，不包含额外 Payload 副本。
    #[serde(default)]
    pub extracted_metadata: BTreeMap<String, String>,
    /// 对最终响应执行的通用断言结果；失败只影响会话结论，不篡改线上响应。
    #[serde(default)]
    pub response_assertions: Vec<ResponseAssertionResultViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ResponseAssertionResultViewModel {
    pub assertion_id: ResponseAssertionId,
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 内存仓储实际保存的会话记录，并提供精确逻辑容量计算。
pub struct SessionRecord {
    pub detail: SessionDetailViewModel,
    pub breakpoint_draft: Option<MessageContentViewModel>,
}

impl SessionRecord {
    pub const ENTITY_FIXED_OVERHEAD_BYTES: u64 = 256;

    pub fn id(&self) -> SessionId {
        self.detail.summary.session_id
    }

    pub fn is_pending(&self) -> bool {
        self.detail.summary.pending_breakpoint
    }

    pub fn logical_bytes(&self) -> u64 {
        let summary = &self.detail.summary;
        let fixed_strings = summary.request_id.len()
            + summary.terminal_ip.len()
            + summary.channel_text.len()
            + summary.method.len()
            + summary.target.len()
            + summary.result.len()
            + self.detail.connection_id.len()
            + self.detail.certificate_fingerprint.len()
            + self.detail.upstream_host.len()
            + self.detail.app_to_proxy_tls.len()
            + self.detail.proxy_to_server_tls.len()
            + self.detail.final_action.len();
        let rule_trace_bytes =
            serde_json::to_vec(&self.detail.rule_trace).map_or(0, |bytes| bytes.len());
        let policy_result_bytes = serde_json::to_vec(&(
            &self.detail.extracted_metadata,
            &self.detail.response_assertions,
        ))
        .map_or(0, |bytes| bytes.len());
        let messages = self
            .detail
            .request
            .as_ref()
            .map_or(0, MessageContentViewModel::logical_bytes)
            + self
                .detail
                .response
                .as_ref()
                .map_or(0, MessageContentViewModel::logical_bytes)
            + self
                .breakpoint_draft
                .as_ref()
                .map_or(0, MessageContentViewModel::logical_bytes);
        Self::ENTITY_FIXED_OVERHEAD_BYTES
            + fixed_strings as u64
            + rule_trace_bytes as u64
            + policy_result_bytes as u64
            + messages
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointState {
    Pending,
    Resolved,
    ClientDisconnected,
    ProxyStopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BreakpointSummaryViewModel {
    pub breakpoint_id: BreakpointId,
    pub session_id: SessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub stage: MessageStage,
    pub title: String,
    pub terminal_ip: String,
    pub channel: ChannelId,
    pub channel_text: String,
    pub method: String,
    pub target: String,
    pub waiting_since: DateTime<Utc>,
    pub certificate_fingerprint_suffix: String,
    pub state: BreakpointState,
    pub state_text: String,
    pub ui_tone: UiTone,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 断点详情：原始报文用于恢复，有效报文用于当前编辑和最终转发。
pub struct BreakpointDetailViewModel {
    pub summary: BreakpointSummaryViewModel,
    pub original: MessageContentViewModel,
    pub effective: MessageContentViewModel,
    pub can_resolve: bool,
    pub resolve_disabled_reason: Option<DisabledReason>,
    pub available_actions: Vec<BreakpointActionOptionViewModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct BreakpointDraft {
    pub breakpoint_id: BreakpointId,
    pub expected_revision: Revision,
    pub message: MessageContentViewModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointDecisionKind {
    ForwardOriginal,
    ForwardModified,
    MockResponse,
    Delay,
    DisconnectBeforeUpstream,
    CustomHttpStatus,
    InvalidJson,
    WrongContentLength,
    Truncate,
    DropResponse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BreakpointActionOptionViewModel {
    pub kind: BreakpointDecisionKind,
    pub label: String,
    pub enabled: bool,
    pub disabled_reason: Option<DisabledReason>,
    pub default_delay_ms: Option<u64>,
    pub default_http_status: Option<u16>,
    pub default_content_length_delta: Option<i64>,
    pub default_truncate_at: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 用户提交的断点决定。所有可选参数最终仍由 Rust 按 `kind` 校验。
pub struct BreakpointDecision {
    pub breakpoint_id: BreakpointId,
    pub expected_revision: Revision,
    pub kind: BreakpointDecisionKind,
    pub message: Option<MessageContentViewModel>,
    pub delay_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub content_length_delta: Option<i64>,
    pub truncate_at: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 可复用于规则、设置、证书和断点的字段校验结果。
pub struct FieldValidationViewModel {
    pub valid: bool,
    pub field_errors: BTreeMap<String, Vec<String>>,
    pub warnings: Vec<String>,
}

pub type BreakpointValidationViewModel = FieldValidationViewModel;
pub type RuleValidationViewModel = FieldValidationViewModel;
pub type CertificateValidationViewModel = FieldValidationViewModel;
pub type SettingsValidationViewModel = FieldValidationViewModel;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleMatchField {
    TerminalIp,
    CertificateFingerprint,
    PathOrRequestType,
    JsonPath { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleMatchFieldKind {
    TerminalIp,
    CertificateFingerprint,
    PathOrRequestType,
    JsonPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleMatchOperator {
    Equals { value: String },
    Contains { value: String },
    Regex { pattern: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleMatchOperatorKind {
    Equals,
    Contains,
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleCondition {
    Field {
        field: RuleMatchField,
        operator: RuleMatchOperator,
    },
    NthHit {
        count: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleConditionKind {
    Field,
    NthHit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleDropResponseMode {
    ReadCompleteResponse,
    CloseAfterRequestWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleTrafficDirection {
    Upstream,
    Downstream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleJitterScope {
    BeforeMessage,
    PerChunk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleTerminalAction {
    RejectTlsHandshake,
    DisconnectBeforeUpstream,
    UpstreamConnectTimeout {
        milliseconds: u64,
    },
    UpstreamWriteTimeout {
        milliseconds: u64,
    },
    UpstreamReadTimeout {
        milliseconds: u64,
    },
    DropUpstreamResponse {
        mode: RuleDropResponseMode,
    },
    MockResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body_bytes: Vec<u8>,
    },
    InvalidJson {
        body_bytes: Vec<u8>,
    },
    IncorrectContentLength {
        delta: i64,
    },
    TruncateResponse {
        bytes: u64,
    },
    DisconnectDuringUpstreamWrite {
        after_bytes: u64,
    },
    DisconnectDuringDownstreamWrite {
        after_bytes: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleAction {
    SetJsonField {
        path: String,
        value_json: String,
    },
    ReplaceBodyText {
        text: String,
    },
    SetHeader {
        name: String,
        value: String,
    },
    Delay {
        milliseconds: u64,
    },
    Jitter {
        minimum_milliseconds: u64,
        maximum_milliseconds: u64,
        scope: RuleJitterScope,
    },
    Throttle {
        bytes_per_second: u64,
        chunk_bytes: u64,
        direction: RuleTrafficDirection,
    },
    Intermittent {
        available_milliseconds: u64,
        blocked_milliseconds: u64,
        direction: RuleTrafficDirection,
    },
    Pause,
    CustomHttpStatus {
        status: u16,
    },
    Terminal {
        action: RuleTerminalAction,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleActionKind {
    SetJsonField,
    ReplaceBodyText,
    SetHeader,
    Delay,
    Jitter,
    Throttle,
    Intermittent,
    Pause,
    CustomHttpStatus,
    RejectTlsHandshake,
    DisconnectBeforeUpstream,
    UpstreamConnectTimeout,
    UpstreamWriteTimeout,
    UpstreamReadTimeout,
    DropUpstreamResponse,
    MockResponse,
    InvalidJson,
    IncorrectContentLength,
    TruncateResponse,
    DisconnectDuringUpstreamWrite,
    DisconnectDuringDownstreamWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuleByteInputViewModel {
    pub bytes: Vec<u8>,
    pub normalized: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuleHeaderInputViewModel {
    pub headers: Vec<(String, String)>,
    pub normalized: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 新建或编辑规则时由展示层提交的输入模型。
pub struct RuleDraft {
    pub rule_id: Option<RuleId>,
    pub expected_revision: Option<Revision>,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub priority: i32,
    pub channel: Option<ChannelId>,
    pub stage: Option<MessageStage>,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    pub one_shot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuleSummaryViewModel {
    pub rule_id: RuleId,
    pub revision: Revision,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub creation_order: u64,
    pub channel_text: String,
    pub stage_text: String,
    pub match_summary: String,
    pub action_summary: String,
    pub hit_count: u64,
    pub last_hit_at: Option<DateTime<Utc>>,
    pub ui_tone: UiTone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct RuleViewModel {
    pub summary: RuleSummaryViewModel,
    pub draft: RuleDraft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FaultParameterKind {
    Boolean,
    Integer,
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FaultParameterValue {
    Boolean(bool),
    Integer(i64),
    Text(String),
    Json(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FaultParameterFieldViewModel {
    pub key: String,
    pub label: String,
    pub description: String,
    pub kind: FaultParameterKind,
    pub required: bool,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
    pub multiline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 故障模拟页展示的产品化模板及参数 schema。
pub struct FaultTemplateViewModel {
    pub template_id: String,
    pub name: String,
    pub stage_text: String,
    pub behavior_text: String,
    pub affected_party_text: String,
    pub default_channel: ChannelId,
    pub default_nth_hit: u32,
    pub default_one_shot: bool,
    pub default_priority: i32,
    pub default_parameters: BTreeMap<String, FaultParameterValue>,
    pub parameter_schema: Vec<FaultParameterFieldViewModel>,
    pub risk_text: String,
    pub ui_tone: UiTone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct FaultConfigurationDraft {
    pub template_id: String,
    pub existing_rule_id: Option<RuleId>,
    pub expected_revision: Option<Revision>,
    pub channel: Option<ChannelId>,
    pub terminal: Option<String>,
    pub target: Option<String>,
    pub nth_hit: Option<u32>,
    pub one_shot: bool,
    pub priority: i32,
    pub parameters: BTreeMap<String, FaultParameterValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ActiveFaultViewModel {
    pub rule_id: RuleId,
    pub template_name: String,
    pub target_summary: String,
    pub priority: i32,
    pub hit_count: u64,
    pub enabled: bool,
    pub status_text: String,
    pub ui_tone: UiTone,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CertificateItemViewModel {
    pub kind: String,
    pub subject: String,
    pub usage: String,
    pub sans: Vec<String>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub sha256_fingerprint: String,
    pub status_text: String,
    pub ui_tone: UiTone,
}

/// 代理监听页面使用的证书引用详情。
/// Workspace 只保存安全引用；证书的主题、SAN、有效期和指纹必须由 Rust 重新解析。
/// 单个引用失效时保留该行并返回 `error_message`，避免一份坏证书阻塞其他证书展示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ListenerCertificateDetailViewModel {
    pub reference_id: CertificateReferenceId,
    pub label: String,
    pub certificate: Option<CertificateItemViewModel>,
    pub error_message: Option<String>,
}

/// 原生导入成功后的安全引用与已解析详情。
/// 导入文件内容、私钥和密码都不会进入 IPC；前端只获得可持久化引用及公开证书元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ListenerCertificateImportViewModel {
    pub reference: CertificateReference,
    pub detail: ListenerCertificateDetailViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CertificateOverviewViewModel {
    pub revision: Revision,
    pub ready: bool,
    pub status_text: String,
    pub ui_tone: UiTone,
    pub items: Vec<CertificateItemViewModel>,
    pub can_initialize: bool,
    pub can_change: bool,
    pub disabled_reason: Option<DisabledReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 设置页中的单个产品通道草稿。
pub struct ChannelSettingsDraft {
    pub id: ChannelId,
    pub display_name: String,
    pub enabled: bool,
    pub port: u16,
    pub upstream_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 设置页提交的前端友好草稿，端口会再转换并执行领域校验。
pub struct SettingsDraft {
    pub expected_revision: Option<Revision>,
    pub bind_address: String,
    pub channels: Vec<ChannelSettingsDraft>,
    pub connect_timeout_seconds: u64,
    pub write_timeout_seconds: u64,
    pub read_timeout_seconds: u64,
    pub rewrite_host: bool,
    pub max_body_bytes: u64,
    pub max_sessions: usize,
    pub max_memory_bytes: u64,
    pub leaf_sans: Vec<String>,
}

impl Default for SettingsDraft {
    fn default() -> Self {
        Self {
            expected_revision: None,
            bind_address: "0.0.0.0".into(),
            channels: Vec::new(),
            connect_timeout_seconds: 70,
            write_timeout_seconds: 70,
            read_timeout_seconds: 70,
            rewrite_host: true,
            max_body_bytes: 4 * 1024 * 1024,
            max_sessions: 500,
            max_memory_bytes: 256 * 1024 * 1024,
            leaf_sans: Vec::new(),
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 设置页展示模型，同时区分已保存值、当前生效值和操作权限。
pub struct SettingsViewModel {
    pub stored: SettingsDraft,
    pub effective: Option<SettingsDraft>,
    pub pending_changes: bool,
    pub requires_restart: bool,
    pub restart_reason: Option<String>,
    pub revision: Revision,
    pub can_write: bool,
    pub disabled_reason: Option<DisabledReason>,
    pub fixed_tls_version: String,
    pub redirects_enabled: bool,
    pub retries_enabled: bool,
    pub payload_policy_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OperationResultViewModel {
    pub success: bool,
    pub cancelled: bool,
    pub message: String,
    pub ui_tone: UiTone,
    pub entity_id: Option<String>,
    pub revision: Option<Revision>,
    pub requires_restart: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Workspace 列表只返回轻量摘要，完整配置仅在用户选择或编辑时加载。
pub struct WorkspaceSummaryViewModel {
    pub id: WorkspaceId,
    pub name: String,
    pub revision: Revision,
    pub listener_count: usize,
    pub enabled_listener_count: usize,
    pub selected: bool,
}

impl WorkspaceSummaryViewModel {
    #[must_use]
    pub fn from_workspace(workspace: &ProxyWorkspace, selected: bool) -> Self {
        Self {
            id: workspace.id,
            name: workspace.name.clone(),
            revision: workspace.revision.get(),
            listener_count: workspace.listeners.len(),
            enabled_listener_count: workspace
                .listeners
                .iter()
                .filter(|listener| listener.enabled)
                .count(),
            selected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Rust Workspace 校验的完整结果。前端不得重复推导安全策略。
pub struct WorkspaceValidationViewModel {
    pub valid: bool,
    pub normalized: ProxyWorkspace,
    pub field_errors: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceChangeKind {
    Created,
    Updated,
    Selected,
    Deleted,
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Workspace 集合变化事件；删除时 `summary` 为空，其余操作携带最新 Rust 摘要。
pub struct WorkspaceChangedViewModel {
    pub workspace_id: WorkspaceId,
    pub kind: WorkspaceChangeKind,
    pub summary: Option<WorkspaceSummaryViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ListenerRuntimeState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Faulted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 单个 Workspace Listener 的运行快照。所有文案与状态均由 Rust 提供。
pub struct ListenerStatusViewModel {
    pub listener_id: ListenerId,
    pub state: ListenerRuntimeState,
    pub state_text: String,
    pub ui_tone: UiTone,
    pub listen_address: String,
    pub fault_reason: Option<String>,
    pub can_start: bool,
    pub can_stop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 对单条已启用 HTTPS 固定 Server 的代理监听执行真实 TCP + TLS 握手后的只读结果。
/// 该模型只包含公开的对端证书元数据，不返回证书字节、客户端私钥或安全引用内容。
/// `client_identity_configured` 只表示本次握手加载了客户端身份；Server 是否强制要求
/// 客户端证书，由握手成功或失败共同判断，前端不能自行推断。
pub struct ListenerUpstreamTlsTestViewModel {
    pub listener_id: ListenerId,
    pub upstream_origin: String,
    pub resolved_address: String,
    pub tls_version: String,
    pub cipher_suite: String,
    pub peer_subject: String,
    pub peer_sha256_fingerprint: String,
    pub hostname_verification_enabled: bool,
    pub client_identity_configured: bool,
    pub elapsed_millis: u64,
    pub message: String,
    pub ui_tone: UiTone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 运行监控中的单个入口行。配置与运行状态由 Rust 合并，前端不推断“缺少状态即停止”。
pub struct ListenerMonitorRowViewModel {
    pub listener_id: ListenerId,
    pub name: String,
    pub kind_text: String,
    pub listen_address: String,
    pub request_destination: String,
    pub state: ListenerRuntimeState,
    pub state_text: String,
    pub ui_tone: UiTone,
    pub fault_reason: Option<String>,
    /// Rust runtime 是否允许当前 Listener 执行启动。
    pub can_start: bool,
    /// Rust runtime 是否允许当前 Listener 执行停止。
    /// `Faulted` 仍可能为 `true`，用于释放 runtime ownership。
    pub can_stop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 当前 Workspace 的入口运行概览，供顶部状态栏与运行监控复用。
pub struct ListenerOverviewViewModel {
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub state_text: String,
    pub ui_tone: UiTone,
    pub total_count: usize,
    pub active_count: usize,
    pub faulted_count: usize,
    pub rows: Vec<ListenerMonitorRowViewModel>,
}

impl WorkspaceValidationViewModel {
    #[must_use]
    pub fn validate(workspace: ProxyWorkspace) -> Self {
        match workspace.validate() {
            Ok(()) => Self {
                valid: true,
                normalized: workspace,
                field_errors: BTreeMap::new(),
            },
            Err(error) => Self {
                valid: false,
                normalized: workspace,
                field_errors: *error.field_errors,
            },
        }
    }
}

impl OperationResultViewModel {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            cancelled: false,
            message: message.into(),
            ui_tone: UiTone::Positive,
            entity_id: None,
            revision: None,
            requires_restart: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 应用启动时一次返回的首屏快照，避免界面自行拼接不一致状态。
pub struct AppBootstrapViewModel {
    pub product_name: String,
    pub proxy: ProxyStatusViewModel,
    pub channel_catalog: Vec<ChannelPresentationViewModel>,
    pub recent_capture: CapturePageViewModel,
    pub pending_breakpoints: Vec<BreakpointSummaryViewModel>,
    pub certificate: CertificateOverviewViewModel,
    pub settings: SettingsViewModel,
    pub event_cursor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SubscriptionAckViewModel {
    pub subscription_id: u64,
    pub accepted_after_event_id: u64,
    pub current_event_id: u64,
    pub snapshot_required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
/// 所有实时事件的封闭集合；适配器可穷举处理，不依赖字符串事件名。
pub enum UiEventPayload {
    WorkspaceChanged(WorkspaceChangedViewModel),
    ListenerStatusChanged(ListenerStatusViewModel),
    RuntimeStatusChanged(Box<ProxyStatusViewModel>),
    ChannelStatusChanged(ChannelStatusViewModel),
    CaptureRowsAdded(Vec<CaptureRowViewModel>),
    SessionUpdated(SessionSummaryViewModel),
    BreakpointQueued(BreakpointSummaryViewModel),
    BreakpointResolved(BreakpointSummaryViewModel),
    RuleHit(RuleSummaryViewModel),
    AndroidVpnStatusChanged(crate::AndroidNetworkStatusViewModel),
    CertificateStatusChanged(CertificateOverviewViewModel),
    SettingsChanged(Box<SettingsViewModel>),
    ResourceWarning { message: String },
    OperationFailed(crate::AppErrorViewModel),
    SnapshotRequired { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 带顺序、周期和实体版本的实时事件信封。
pub struct UiEventEnvelope {
    pub event_id: u64,
    pub runtime_epoch: Option<RuntimeEpoch>,
    pub occurred_at: DateTime<Utc>,
    pub entity_id: Option<String>,
    pub entity_revision: Option<Revision>,
    pub payload: UiEventPayload,
}

impl UiEventEnvelope {
    pub fn logical_bytes(&self) -> u64 {
        serde_json::to_vec(self).map_or(0, |bytes| bytes.len() as u64)
    }
}

#[cfg(test)]
mod workspace_tests {
    use super::*;

    #[test]
    fn workspace_summary_is_computed_by_rust() {
        let workspace = ProxyWorkspace::default();
        let summary = WorkspaceSummaryViewModel::from_workspace(&workspace, true);
        assert_eq!(summary.id, workspace.id);
        assert_eq!(summary.listener_count, 1);
        assert_eq!(summary.enabled_listener_count, 0);
        assert!(summary.selected);
    }

    #[test]
    fn workspace_validation_returns_rust_field_errors() {
        let mut workspace = ProxyWorkspace::default();
        workspace.name.clear();
        let validation = WorkspaceValidationViewModel::validate(workspace);
        assert!(!validation.valid);
        assert!(validation.field_errors.contains_key("name"));
    }
}
