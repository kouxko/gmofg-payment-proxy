use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ChannelId, MessageStage, Revision, RuleId, UiTone};

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

/// 单个动作在指定 HTTP 阶段中的可用能力。
///
/// 前端只渲染这里返回的动作，不再根据动作名称推断请求、响应或 TLS 语义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuleActionCapabilityViewModel {
    pub kind: RuleActionKind,
    pub terminal: bool,
    pub traffic_direction: Option<RuleTrafficDirection>,
}

/// HTTP 规则编辑器针对一个阶段的完整能力表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuleStageCapabilityViewModel {
    pub stage: MessageStage,
    pub match_field_kinds: Vec<RuleMatchFieldKind>,
    pub actions: Vec<RuleActionCapabilityViewModel>,
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
