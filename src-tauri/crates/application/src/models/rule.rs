use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{Revision, RuleId, UiTone};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleMatchFieldKind {
    TerminalIp,
    CertificateFingerprint,
    Method,
    RequestTarget,
    Header,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleMatchOperatorKind {
    Equals,
    Contains,
    StartsWith,
    EndsWith,
    Wildcard,
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
    CustomHttpStatus,
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
    pub parameters_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleMatchSelectorKind {
    HeaderNamePointer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuleMatchFieldCapabilityViewModel {
    pub kind: RuleMatchFieldKind,
    pub operators: Vec<RuleMatchOperatorKind>,
    pub selector: Option<RuleMatchSelectorKind>,
}

/// HTTP 规则编辑器针对一个阶段的完整能力表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RuleStageCapabilityViewModel {
    pub stage: crate::RuleStage,
    pub match_fields: Vec<RuleMatchFieldCapabilityViewModel>,
    pub actions: Vec<RuleActionCapabilityViewModel>,
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
