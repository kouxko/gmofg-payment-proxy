use crate::{
    ChannelId, DomainError, ErrorCode, MessageStage, Revision, RuleId, RuntimeEpoch,
    TerminalIdentity,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use uuid::Uuid;

/// 一条规则组合允许累积的最大延迟：10 分钟。
pub const MAX_TOTAL_DELAY_MS: u64 = 600_000;
/// 限速动作允许设置的最高速率。
pub const MAX_THROTTLE_BYTES_PER_SECOND: u64 = 100 * 1024 * 1024;
/// 弱网动作处理的最大分块大小。
pub const MAX_TRAFFIC_CHUNK_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum MatchField {
    TerminalIp,
    CertificateFingerprint,
    PathOrRequestType,
    JsonPath(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum MatchOperator {
    Equals(String),
    Contains(String),
    Regex(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum MatchCondition {
    Field {
        field: MatchField,
        operator: MatchOperator,
    },
    NthHit(u64),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum DropResponseMode {
    ReadCompleteResponse,
    CloseAfterRequestWrite,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum TrafficDirection {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum JitterScope {
    BeforeMessage,
    PerChunk,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum TerminalAction {
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
        mode: DropResponseMode,
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

impl TerminalAction {
    pub fn validate_for_body(&self, body_len: usize) -> Result<(), DomainError> {
        let (bytes, field, message) = match self {
            Self::TruncateResponse { bytes } => (*bytes, "bytes", "截断长度"),
            Self::DisconnectDuringUpstreamWrite { after_bytes }
            | Self::DisconnectDuringDownstreamWrite { after_bytes } => {
                (*after_bytes, "after_bytes", "断连偏移")
            }
            _ => return Ok(()),
        };
        let bytes = usize::try_from(bytes).map_err(|_| {
            DomainError::new(ErrorCode::RuleInvalid, format!("{message}超出平台范围"))
                .with_field_error(field, format!("{message}非法"))
        })?;
        if body_len == 0 || bytes >= body_len {
            return Err(DomainError::new(
                ErrorCode::RuleInvalid,
                format!("{message}必须小于 Body 长度"),
            )
            .with_field_error(field, "必须位于 0..body_len-1"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum RuleAction {
    SetJsonField {
        path: String,
        #[specta(type = specta_typescript::Unknown<Value>)]
        value: Value,
    },
    ReplaceBodyText(String),
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
        scope: JitterScope,
    },
    Throttle {
        bytes_per_second: u64,
        chunk_bytes: u64,
        direction: TrafficDirection,
    },
    Intermittent {
        available_milliseconds: u64,
        blocked_milliseconds: u64,
        direction: TrafficDirection,
    },
    Pause,
    CustomHttpStatus {
        status: u16,
    },
    Terminal(TerminalAction),
}

impl RuleAction {
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleDraft {
    pub expected_revision: Option<Revision>,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub priority: u32,
    pub created_order: u64,
    pub channel: Option<ChannelId>,
    pub stage: MessageStage,
    pub conditions: Vec<MatchCondition>,
    pub actions: Vec<RuleAction>,
    pub one_shot: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct Rule {
    pub id: RuleId,
    pub revision: Revision,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub priority: u32,
    pub created_order: u64,
    pub channel: Option<ChannelId>,
    pub stage: MessageStage,
    pub conditions: Vec<MatchCondition>,
    pub actions: Vec<RuleAction>,
    pub one_shot: bool,
    pub hit_count: u64,
    pub last_hit_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, Type)]
pub struct RuleRevisionSignature {
    pub rule_id: RuleId,
    pub revision: Revision,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleSetSignature {
    pub entries: Vec<RuleRevisionSignature>,
}

impl RuleSetSignature {
    #[must_use]
    pub fn from_rules(rules: &[Rule]) -> Self {
        let mut entries = rules
            .iter()
            .map(|rule| RuleRevisionSignature {
                rule_id: rule.id,
                revision: rule.revision,
            })
            .collect::<Vec<_>>();
        entries.sort_unstable();
        Self { entries }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleRuntimeSnapshot {
    pub collection_id: Option<Uuid>,
    pub collection_revision: u64,
    pub signature: RuleSetSignature,
    pub rules: Vec<Rule>,
    pub execution_order: Vec<RuleId>,
}

impl RuleRuntimeSnapshot {
    #[must_use]
    pub fn new(rules: Vec<Rule>) -> Self {
        Self::with_collection_revision(0, rules)
    }

    #[must_use]
    pub fn with_collection_revision(collection_revision: u64, rules: Vec<Rule>) -> Self {
        Self::with_collection_identity(None, collection_revision, rules)
    }

    #[must_use]
    pub fn with_collection_identity(
        collection_id: Option<Uuid>,
        collection_revision: u64,
        rules: Vec<Rule>,
    ) -> Self {
        Self {
            collection_id,
            collection_revision,
            signature: RuleSetSignature::from_rules(&rules),
            rules,
            execution_order: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_collection_identity_and_order(
        collection_id: Option<Uuid>,
        collection_revision: u64,
        rules: Vec<Rule>,
        execution_order: Vec<RuleId>,
    ) -> Self {
        Self {
            collection_id,
            collection_revision,
            signature: RuleSetSignature::from_rules(&rules),
            rules,
            execution_order,
        }
    }
}

impl Rule {
    pub fn create(draft: RuleDraft) -> Result<Self, DomainError> {
        super::validate_rule_draft(&draft)?;
        Ok(Self {
            id: RuleId::new(),
            revision: Revision::INITIAL,
            name: draft.name,
            description: draft.description,
            enabled: draft.enabled,
            priority: draft.priority,
            created_order: draft.created_order,
            channel: draft.channel,
            stage: draft.stage,
            conditions: draft.conditions,
            actions: draft.actions,
            one_shot: draft.one_shot,
            hit_count: 0,
            last_hit_at: None,
        })
    }

    pub(super) fn apply_draft(&mut self, draft: RuleDraft) {
        self.name = draft.name;
        self.description = draft.description;
        self.enabled = draft.enabled;
        self.priority = draft.priority;
        self.created_order = draft.created_order;
        self.channel = draft.channel;
        self.stage = draft.stage;
        self.conditions = draft.conditions;
        self.actions = draft.actions;
        self.one_shot = draft.one_shot;
        self.revision = self.revision.next();
    }
}

#[derive(Clone, Debug)]
pub struct MatchContext<'a> {
    pub runtime_epoch: RuntimeEpoch,
    pub channel: ChannelId,
    pub stage: MessageStage,
    pub terminal: &'a TerminalIdentity,
    pub path_or_request_type: Option<&'a str>,
    pub json_body: Option<&'a Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleTrace {
    pub rule_id: RuleId,
    pub matched: bool,
    pub reason: String,
    pub actions: Vec<RuleAction>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleEvaluation {
    pub traces: Vec<RuleTrace>,
    pub composed_actions: Vec<RuleAction>,
    pub terminal_action: Option<TerminalAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleConflictWarning {
    pub code: ErrorCode,
    pub shadowing_rule_id: RuleId,
    pub shadowed_rule_id: RuleId,
    pub message: String,
}
