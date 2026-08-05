use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    BreakpointId, ChannelId, DisabledReason, MessageContentViewModel, MessageStage, Revision,
    RuntimeEpoch, SessionId, UiTone,
};

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
