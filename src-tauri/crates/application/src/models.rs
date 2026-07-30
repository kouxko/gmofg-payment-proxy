use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use uuid::Uuid;

pub type RuntimeEpoch = Uuid;
pub type Revision = u64;
pub type SessionId = Uuid;
pub type BreakpointId = Uuid;
pub type RuleId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UiTone {
    Neutral,
    Info,
    Positive,
    Warning,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
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
pub enum ChannelKind {
    Transaction,
    Dll,
}

impl ChannelKind {
    pub fn display_zh(self) -> &'static str {
        match self {
            Self::Transaction => "交易",
            Self::Dll => "DLL",
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
    pub kind: ChannelKind,
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
pub struct CaptureQuery {
    pub keyword: Option<String>,
    pub terminal_ip: Option<String>,
    pub channel: Option<ChannelKind>,
    pub stage: Option<MessageStage>,
    pub result: Option<String>,
    pub rule_id: Option<RuleId>,
    /// When present, returns only retained rows newer than this cursor.
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
pub struct CaptureRowViewModel {
    pub event_id: u64,
    pub runtime_epoch: RuntimeEpoch,
    pub session_id: SessionId,
    pub occurred_at: DateTime<Utc>,
    pub terminal_ip: String,
    pub channel: ChannelKind,
    pub channel_text: String,
    pub stage: MessageStage,
    pub stage_text: String,
    pub method: String,
    pub target: String,
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
pub struct MessageContentViewModel {
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
        Self::ENTITY_FIXED_OVERHEAD_BYTES + (headers + self.body_bytes.len()) as u64
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
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionQuery {
    pub keyword: Option<String>,
    pub terminal_ip: Option<String>,
    pub channel: Option<ChannelKind>,
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
    pub channel: ChannelKind,
    pub method: String,
    pub target: String,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
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
    pub channel: ChannelKind,
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
        shift_jis_body: Vec<u8>,
    },
    InvalidJson {
        shift_jis_body: Vec<u8>,
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
pub struct RuleDraft {
    pub rule_id: Option<RuleId>,
    pub expected_revision: Option<Revision>,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub priority: i32,
    pub channel: Option<ChannelKind>,
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
pub struct FaultTemplateViewModel {
    pub template_id: String,
    pub name: String,
    pub stage_text: String,
    pub behavior_text: String,
    pub affected_party_text: String,
    pub default_channel: ChannelKind,
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
    pub channel: Option<ChannelKind>,
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
pub struct SettingsDraft {
    pub expected_revision: Option<Revision>,
    pub bind_address: String,
    pub transaction_enabled: bool,
    pub transaction_port: u16,
    pub dll_enabled: bool,
    pub dll_port: u16,
    pub upstream_transaction_url: String,
    pub upstream_dll_url: String,
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
            transaction_enabled: true,
            transaction_port: 16_627,
            dll_enabled: true,
            dll_port: 16_127,
            upstream_transaction_url: String::new(),
            upstream_dll_url: String::new(),
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
pub struct AppBootstrapViewModel {
    pub proxy: ProxyStatusViewModel,
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
pub enum UiEventPayload {
    RuntimeStatusChanged(Box<ProxyStatusViewModel>),
    ChannelStatusChanged(ChannelStatusViewModel),
    CaptureRowsAdded(Vec<CaptureRowViewModel>),
    SessionUpdated(SessionSummaryViewModel),
    BreakpointQueued(BreakpointSummaryViewModel),
    BreakpointResolved(BreakpointSummaryViewModel),
    RuleHit(RuleSummaryViewModel),
    CertificateStatusChanged(CertificateOverviewViewModel),
    SettingsChanged(Box<SettingsViewModel>),
    ResourceWarning { message: String },
    OperationFailed(crate::AppErrorViewModel),
    SnapshotRequired { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
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
