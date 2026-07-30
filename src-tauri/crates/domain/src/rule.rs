use crate::{
    ChannelKind, DomainError, ErrorCode, JsonPath, MessageStage, Revision, RuleId, RuntimeEpoch,
    TerminalIdentity,
};
use chrono::{DateTime, Utc};
use encoding_rs::SHIFT_JIS;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::collections::HashMap;

pub const MAX_TOTAL_DELAY_MS: u64 = 600_000;
pub const MAX_THROTTLE_BYTES_PER_SECOND: u64 = 100 * 1024 * 1024;
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
    pub channel: Option<ChannelKind>,
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
    pub channel: Option<ChannelKind>,
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
    pub collection_revision: u64,
    pub signature: RuleSetSignature,
    pub rules: Vec<Rule>,
}

impl RuleRuntimeSnapshot {
    #[must_use]
    pub fn new(rules: Vec<Rule>) -> Self {
        Self::with_collection_revision(0, rules)
    }

    #[must_use]
    pub fn with_collection_revision(collection_revision: u64, rules: Vec<Rule>) -> Self {
        Self {
            collection_revision,
            signature: RuleSetSignature::from_rules(&rules),
            rules,
        }
    }
}

impl Rule {
    pub fn create(draft: RuleDraft) -> Result<Self, DomainError> {
        validate_rule_draft(&draft)?;
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

    fn apply_draft(&mut self, draft: RuleDraft) {
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
    pub channel: ChannelKind,
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CounterKey {
    rule_id: RuleId,
    source_ip: String,
    certificate_sha256: String,
}

#[derive(Clone, Debug, Default)]
pub struct RuleEngine {
    runtime_epoch: Option<RuntimeEpoch>,
    rules: Vec<Rule>,
    counters: HashMap<CounterKey, u64>,
}

impl RuleEngine {
    #[must_use]
    pub fn new(runtime_epoch: RuntimeEpoch, rules: Vec<Rule>) -> Self {
        Self {
            runtime_epoch: Some(runtime_epoch),
            rules,
            counters: HashMap::new(),
        }
    }

    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn restart(&mut self, runtime_epoch: RuntimeEpoch) {
        self.runtime_epoch = Some(runtime_epoch);
        self.counters.clear();
        for rule in &mut self.rules {
            rule.hit_count = 0;
            rule.last_hit_at = None;
        }
    }

    /// Applies a fresh persisted rule snapshot while retaining per-terminal
    /// Nth-hit counters for rules whose matching semantics remain unchanged.
    pub fn reconcile(&mut self, rules: Vec<Rule>) {
        let reset_ids = rules
            .iter()
            .filter_map(|next| {
                let previous = self.rules.iter().find(|rule| rule.id == next.id);
                match previous {
                    Some(previous)
                        if previous.conditions == next.conditions
                            && (previous.enabled || !next.enabled) =>
                    {
                        None
                    }
                    _ => Some(next.id),
                }
            })
            .collect::<Vec<_>>();
        self.counters.retain(|key, _| {
            rules.iter().any(|rule| rule.id == key.rule_id) && !reset_ids.contains(&key.rule_id)
        });
        self.rules = rules;
    }

    pub fn save(&mut self, id: RuleId, draft: RuleDraft) -> Result<Revision, DomainError> {
        validate_rule_draft(&draft)?;
        let (must_reset, revision) = {
            let rule = self
                .rules
                .iter_mut()
                .find(|rule| rule.id == id)
                .ok_or_else(|| DomainError::new(ErrorCode::RuleInvalid, "规则不存在"))?;
            let expected = draft.expected_revision.ok_or_else(|| {
                DomainError::new(ErrorCode::RevisionConflict, "修改规则必须提供当前 revision")
            })?;
            rule.revision.verify(expected)?;
            let must_reset =
                rule.conditions != draft.conditions || (!rule.enabled && draft.enabled);
            rule.apply_draft(draft);
            (must_reset, rule.revision)
        };
        if must_reset {
            self.reset_rule_hits(id);
        }
        Ok(revision)
    }

    pub fn toggle(
        &mut self,
        id: RuleId,
        expected_revision: Revision,
        enabled: bool,
    ) -> Result<Revision, DomainError> {
        let rule = self
            .rules
            .iter_mut()
            .find(|rule| rule.id == id)
            .ok_or_else(|| DomainError::new(ErrorCode::RuleInvalid, "规则不存在"))?;
        rule.revision.verify(expected_revision)?;
        let reset = !rule.enabled && enabled;
        rule.enabled = enabled;
        rule.revision = rule.revision.next();
        let revision = rule.revision;
        if reset {
            self.reset_rule_hits(id);
        }
        Ok(revision)
    }

    pub fn evaluate(&mut self, context: &MatchContext<'_>, now: DateTime<Utc>) -> RuleEvaluation {
        if self.runtime_epoch != Some(context.runtime_epoch) {
            self.restart(context.runtime_epoch);
        }

        // ENGINE-002: this clone is the immutable rule snapshot for one message.
        let mut snapshot = self.rules.clone();
        snapshot.sort_by_key(|rule| (rule.priority, rule.created_order, rule.id));
        let mut evaluation = RuleEvaluation::default();
        let mut hit_ids = Vec::new();

        for rule in snapshot.iter().filter(|rule| rule.enabled) {
            if rule
                .channel
                .is_some_and(|channel| channel != context.channel)
                || rule.stage != context.stage
            {
                continue;
            }
            let match_result = self.matches_rule(rule, context);
            match match_result {
                Ok(true) => {
                    hit_ids.push(rule.id);
                    let mut executed = Vec::new();
                    for action in &rule.actions {
                        executed.push(action.clone());
                        evaluation.composed_actions.push(action.clone());
                        if let RuleAction::Terminal(terminal) = action {
                            evaluation.terminal_action = Some(terminal.clone());
                            break;
                        }
                    }
                    evaluation.traces.push(RuleTrace {
                        rule_id: rule.id,
                        matched: true,
                        reason: "全部匹配条件满足".into(),
                        actions: executed,
                    });
                    if evaluation.terminal_action.is_some() {
                        break;
                    }
                }
                Ok(false) => evaluation.traces.push(RuleTrace {
                    rule_id: rule.id,
                    matched: false,
                    reason: "匹配条件不满足".into(),
                    actions: Vec::new(),
                }),
                Err(reason) => evaluation.traces.push(RuleTrace {
                    rule_id: rule.id,
                    matched: false,
                    reason,
                    actions: Vec::new(),
                }),
            }
        }

        for id in hit_ids {
            if let Some(rule) = self.rules.iter_mut().find(|rule| rule.id == id) {
                rule.hit_count = rule.hit_count.saturating_add(1);
                rule.last_hit_at = Some(now);
                if rule.one_shot {
                    rule.enabled = false;
                    rule.revision = rule.revision.next();
                }
            }
        }
        evaluation
    }

    #[must_use]
    pub fn conflict_warnings(&self) -> Vec<RuleConflictWarning> {
        let mut sorted: Vec<&Rule> = self.rules.iter().filter(|rule| rule.enabled).collect();
        sorted.sort_by_key(|rule| (rule.priority, rule.created_order, rule.id));
        let mut warnings = Vec::new();
        for (index, higher) in sorted.iter().enumerate() {
            if !higher.actions.iter().any(RuleAction::is_terminal) {
                continue;
            }
            for lower in sorted.iter().skip(index + 1) {
                if higher.stage == lower.stage
                    && (higher.channel.is_none() || higher.channel == lower.channel)
                    && higher
                        .conditions
                        .iter()
                        .all(|condition| lower.conditions.contains(condition))
                {
                    warnings.push(RuleConflictWarning {
                        code: ErrorCode::RuleConflictWarning,
                        shadowing_rule_id: higher.id,
                        shadowed_rule_id: lower.id,
                        message: format!("规则“{}”可能遮蔽规则“{}”", higher.name, lower.name),
                    });
                }
            }
        }
        warnings
    }

    fn reset_rule_hits(&mut self, id: RuleId) {
        self.counters.retain(|key, _| key.rule_id != id);
        if let Some(rule) = self.rules.iter_mut().find(|rule| rule.id == id) {
            rule.hit_count = 0;
            rule.last_hit_at = None;
        }
    }

    fn matches_rule(&mut self, rule: &Rule, context: &MatchContext<'_>) -> Result<bool, String> {
        for condition in rule
            .conditions
            .iter()
            .filter(|condition| !matches!(condition, MatchCondition::NthHit(_)))
        {
            if !matches_condition(condition, context)? {
                return Ok(false);
            }
        }

        let nth_values: Vec<u64> = rule
            .conditions
            .iter()
            .filter_map(|condition| match condition {
                MatchCondition::NthHit(value) => Some(*value),
                MatchCondition::Field { .. } => None,
            })
            .collect();
        if nth_values.is_empty() {
            return Ok(true);
        }

        let key = CounterKey {
            rule_id: rule.id,
            source_ip: context.terminal.source_ip.clone(),
            certificate_sha256: context.terminal.certificate_sha256.clone(),
        };
        let count = self.counters.entry(key).or_default();
        *count = count.saturating_add(1);
        Ok(nth_values.iter().all(|nth| *nth == *count))
    }
}

pub fn validate_rule_draft(draft: &RuleDraft) -> Result<(), DomainError> {
    let mut error = DomainError::new(ErrorCode::RuleInvalid, "规则配置非法");
    if draft.name.trim().is_empty() {
        error = error.with_field_error("name", "规则名称不能为空");
    }
    if draft.actions.is_empty() {
        error = error.with_field_error("actions", "至少配置一个动作");
    }
    for (index, condition) in draft.conditions.iter().enumerate() {
        match condition {
            MatchCondition::Field {
                field: MatchField::JsonPath(path),
                ..
            } if JsonPath::parse(path).is_err() => {
                error =
                    error.with_field_error(format!("conditions.{index}.path"), "JSON 字段路径非法");
            }
            MatchCondition::Field {
                operator: MatchOperator::Regex(pattern),
                ..
            } if Regex::new(pattern).is_err() => {
                error =
                    error.with_field_error(format!("conditions.{index}.regex"), "正则表达式非法");
            }
            MatchCondition::NthHit(0) => {
                error = error.with_field_error(
                    format!("conditions.{index}.nth_hit"),
                    "第 N 次命中必须大于 0",
                );
            }
            MatchCondition::Field { .. } | MatchCondition::NthHit(_) => {}
        }
    }

    let total_delay = draft.actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(match action {
            RuleAction::Delay { milliseconds } => *milliseconds,
            RuleAction::Jitter {
                maximum_milliseconds,
                ..
            } => *maximum_milliseconds,
            _ => 0,
        })
    });
    if total_delay > MAX_TOTAL_DELAY_MS {
        error = error.with_field_error("actions", "累计延迟不得超过 600000 毫秒");
    }

    validate_actions(draft, &mut error);

    if draft.stage == MessageStage::TlsHandshake
        && draft.conditions.iter().any(|condition| {
            !matches!(
                condition,
                MatchCondition::Field {
                    field: MatchField::CertificateFingerprint,
                    ..
                } | MatchCondition::NthHit(_)
            )
        })
    {
        error = error.with_field_error(
            "conditions",
            "TLS 握手拒绝只允许通道、客户端证书和第 N 次命中条件",
        );
    }

    if error.field_errors.is_empty() {
        Ok(())
    } else {
        Err(error)
    }
}

#[allow(clippy::too_many_lines)]
fn validate_actions(draft: &RuleDraft, error: &mut DomainError) {
    let terminal_positions: Vec<usize> = draft
        .actions
        .iter()
        .enumerate()
        .filter_map(|(index, action)| action.is_terminal().then_some(index))
        .collect();
    if terminal_positions.len() > 1
        || terminal_positions
            .first()
            .is_some_and(|&index| index + 1 != draft.actions.len())
    {
        push_field_error(error, "actions", "终止动作必须唯一且位于动作列表末尾");
    }

    for (index, action) in draft.actions.iter().enumerate() {
        if !action_compatible(draft.stage, action) {
            push_field_error(error, format!("actions.{index}"), "动作与规则阶段不兼容");
        }
        if let RuleAction::CustomHttpStatus { status } = action
            && !(100..=599).contains(status)
        {
            push_field_error(
                error,
                format!("actions.{index}.status"),
                "HTTP 状态码必须位于 100..599",
            );
        }
        match action {
            RuleAction::Jitter {
                minimum_milliseconds,
                maximum_milliseconds,
                ..
            } => {
                if minimum_milliseconds > maximum_milliseconds {
                    push_field_error(
                        error,
                        format!("actions.{index}.minimum_milliseconds"),
                        "最小抖动不得大于最大抖动",
                    );
                }
                if *maximum_milliseconds > MAX_TOTAL_DELAY_MS {
                    push_field_error(
                        error,
                        format!("actions.{index}.maximum_milliseconds"),
                        "单次抖动不得超过 600000 毫秒",
                    );
                }
            }
            RuleAction::Throttle {
                bytes_per_second,
                chunk_bytes,
                ..
            } => {
                if !(1..=MAX_THROTTLE_BYTES_PER_SECOND).contains(bytes_per_second) {
                    push_field_error(
                        error,
                        format!("actions.{index}.bytes_per_second"),
                        "限速必须位于 1..104857600 B/s",
                    );
                }
                if !(1..=MAX_TRAFFIC_CHUNK_BYTES).contains(chunk_bytes) {
                    push_field_error(
                        error,
                        format!("actions.{index}.chunk_bytes"),
                        "分块大小必须位于 1..1048576 字节",
                    );
                }
            }
            RuleAction::Intermittent {
                available_milliseconds,
                blocked_milliseconds,
                ..
            } => {
                if !(1..=MAX_TOTAL_DELAY_MS).contains(available_milliseconds) {
                    push_field_error(
                        error,
                        format!("actions.{index}.available_milliseconds"),
                        "可用窗口必须位于 1..600000 毫秒",
                    );
                }
                if !(1..=MAX_TOTAL_DELAY_MS).contains(blocked_milliseconds) {
                    push_field_error(
                        error,
                        format!("actions.{index}.blocked_milliseconds"),
                        "阻断窗口必须位于 1..600000 毫秒",
                    );
                }
            }
            RuleAction::SetJsonField { .. }
            | RuleAction::ReplaceBodyText(_)
            | RuleAction::SetHeader { .. }
            | RuleAction::Delay { .. }
            | RuleAction::Pause
            | RuleAction::CustomHttpStatus { .. }
            | RuleAction::Terminal(_) => {}
        }
        if let RuleAction::Terminal(TerminalAction::IncorrectContentLength { delta }) = action
            && *delta == 0
        {
            push_field_error(
                error,
                format!("actions.{index}.delta"),
                "错误长度差值不能为 0",
            );
        }
        if let RuleAction::Terminal(
            TerminalAction::UpstreamConnectTimeout { milliseconds }
            | TerminalAction::UpstreamWriteTimeout { milliseconds }
            | TerminalAction::UpstreamReadTimeout { milliseconds },
        ) = action
            && *milliseconds == 0
        {
            push_field_error(
                error,
                format!("actions.{index}.milliseconds"),
                "故障超时必须大于 0 毫秒",
            );
        }
        if let RuleAction::Terminal(TerminalAction::MockResponse { status, .. }) = action
            && !(100..=599).contains(status)
        {
            push_field_error(
                error,
                format!("actions.{index}.status"),
                "Mock HTTP 状态码必须位于 100..599",
            );
        }
        validate_action_content(error, index, action);
    }
}

fn validate_action_content(error: &mut DomainError, index: usize, action: &RuleAction) {
    match action {
        RuleAction::SetJsonField { path, value } => {
            if JsonPath::parse(path).is_err() {
                push_field_error(error, format!("actions.{index}.path"), "JSON 字段路径非法");
            }
            if serde_json::to_string(value).is_ok_and(|text| !is_strict_shift_jis_text(&text)) {
                push_field_error(
                    error,
                    format!("actions.{index}.value_json"),
                    "JSON 值包含无法无损编码为 Shift-JIS 的字符",
                );
            }
        }
        RuleAction::ReplaceBodyText(text) if !is_strict_shift_jis_text(text) => {
            push_field_error(
                error,
                format!("actions.{index}.text"),
                "Body 文本包含无法无损编码为 Shift-JIS 的字符",
            );
        }
        RuleAction::SetHeader { name, value } => {
            validate_header(error, &format!("actions.{index}"), name, value, false);
        }
        RuleAction::Terminal(TerminalAction::MockResponse {
            headers,
            shift_jis_body,
            ..
        }) => {
            for (header_index, (name, value)) in headers.iter().enumerate() {
                validate_header(
                    error,
                    &format!("actions.{index}.headers.{header_index}"),
                    name,
                    value,
                    false,
                );
            }
            validate_mock_json_body(error, index, shift_jis_body);
        }
        RuleAction::Terminal(TerminalAction::InvalidJson { shift_jis_body }) => {
            validate_invalid_json_body(error, index, shift_jis_body);
        }
        _ => {}
    }
}

fn validate_mock_json_body(error: &mut DomainError, index: usize, body: &[u8]) {
    match decode_strict_shift_jis(body) {
        Some(text) if serde_json::from_str::<Value>(&text).is_ok() => {}
        Some(_) => push_field_error(
            error,
            format!("actions.{index}.shift_jis_body"),
            "Mock Body 必须是有效的 Shift-JIS JSON",
        ),
        None => push_field_error(
            error,
            format!("actions.{index}.shift_jis_body"),
            "Mock Body 包含非法 Shift-JIS 字节",
        ),
    }
}

fn validate_invalid_json_body(error: &mut DomainError, index: usize, body: &[u8]) {
    match decode_strict_shift_jis(body) {
        Some(text) if serde_json::from_str::<Value>(&text).is_err() => {}
        Some(_) => push_field_error(
            error,
            format!("actions.{index}.shift_jis_body"),
            "非法 JSON 故障的 Body 不能是有效 JSON",
        ),
        None => push_field_error(
            error,
            format!("actions.{index}.shift_jis_body"),
            "非法 JSON 故障仍必须是有效 Shift-JIS 字节",
        ),
    }
}

fn validate_header(
    error: &mut DomainError,
    field_prefix: &str,
    name: &str,
    value: &str,
    allow_managed_length: bool,
) {
    if !is_valid_header_name(name) {
        push_field_error(
            error,
            format!("{field_prefix}.name"),
            "Header 名称必须是非空 ASCII token",
        );
    }
    if !is_valid_header_value(value) {
        push_field_error(
            error,
            format!("{field_prefix}.value"),
            "Header 值不能包含换行、NUL 或其他非法控制字符",
        );
    }
    if !allow_managed_length
        && matches!(
            name.trim().to_ascii_lowercase().as_str(),
            "content-length"
                | "transfer-encoding"
                | "connection"
                | "proxy-connection"
                | "keep-alive"
                | "upgrade"
                | "te"
                | "trailer"
        )
    {
        push_field_error(
            error,
            format!("{field_prefix}.name"),
            "该 Header 由 Rust 转发管线统一管理，规则不得直接设置",
        );
    }
}

fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn is_valid_header_value(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte == b'\t' || byte >= 0x20 && byte != 0x7f)
}

fn is_strict_shift_jis_text(text: &str) -> bool {
    let (_, _, had_errors) = SHIFT_JIS.encode(text);
    !had_errors
}

fn decode_strict_shift_jis(bytes: &[u8]) -> Option<String> {
    let (decoded, had_errors) = SHIFT_JIS.decode_without_bom_handling(bytes);
    (!had_errors).then(|| decoded.into_owned())
}

fn push_field_error(error: &mut DomainError, field: impl Into<String>, message: impl Into<String>) {
    error
        .field_errors
        .entry(field.into())
        .or_default()
        .push(message.into());
}

fn action_compatible(stage: MessageStage, action: &RuleAction) -> bool {
    match action {
        RuleAction::SetJsonField { .. }
        | RuleAction::ReplaceBodyText(_)
        | RuleAction::SetHeader { .. }
        | RuleAction::Delay { .. }
        | RuleAction::Jitter { .. }
        | RuleAction::Pause => stage != MessageStage::TlsHandshake,
        RuleAction::Throttle { direction, .. } | RuleAction::Intermittent { direction, .. } => {
            matches!(
                (stage, direction),
                (MessageStage::Request, TrafficDirection::Upstream)
                    | (MessageStage::Response, TrafficDirection::Downstream)
            )
        }
        RuleAction::CustomHttpStatus { .. } => stage == MessageStage::Response,
        RuleAction::Terminal(terminal) => match terminal {
            TerminalAction::RejectTlsHandshake => stage == MessageStage::TlsHandshake,
            TerminalAction::DisconnectBeforeUpstream
            | TerminalAction::UpstreamConnectTimeout { .. }
            | TerminalAction::UpstreamWriteTimeout { .. }
            | TerminalAction::UpstreamReadTimeout { .. }
            | TerminalAction::DropUpstreamResponse { .. }
            | TerminalAction::MockResponse { .. }
            | TerminalAction::DisconnectDuringUpstreamWrite { .. } => {
                stage == MessageStage::Request
            }
            TerminalAction::InvalidJson { .. }
            | TerminalAction::IncorrectContentLength { .. }
            | TerminalAction::TruncateResponse { .. }
            | TerminalAction::DisconnectDuringDownstreamWrite { .. } => {
                stage == MessageStage::Response
            }
        },
    }
}

fn matches_condition(
    condition: &MatchCondition,
    context: &MatchContext<'_>,
) -> Result<bool, String> {
    let MatchCondition::Field { field, operator } = condition else {
        return Ok(true);
    };
    let value = match field {
        MatchField::TerminalIp => context.terminal.source_ip.clone(),
        MatchField::CertificateFingerprint => context.terminal.certificate_sha256.clone(),
        MatchField::PathOrRequestType => {
            context.path_or_request_type.unwrap_or_default().to_owned()
        }
        MatchField::JsonPath(path) => {
            let Some(json) = context.json_body else {
                return Err("Body 不是可解析 JSON，JSON 字段条件不匹配".into());
            };
            let parsed = JsonPath::parse(path)
                .map_err(|_| "规则包含未通过保存校验的 JSON 字段路径".to_owned())?;
            let Some(value) = parsed.resolve(json) else {
                return Err(format!("JSON 字段路径不存在：{path}"));
            };
            json_scalar(value)
        }
    };
    Ok(match operator {
        MatchOperator::Equals(expected) => value == *expected,
        MatchOperator::Contains(fragment) => value.contains(fragment),
        MatchOperator::Regex(pattern) => Regex::new(pattern)
            .map_err(|_| "规则包含未通过保存校验的正则".to_owned())?
            .is_match(&value),
    })
}

fn json_scalar(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn draft(
        stage: MessageStage,
        conditions: Vec<MatchCondition>,
        actions: Vec<RuleAction>,
    ) -> RuleDraft {
        RuleDraft {
            expected_revision: None,
            name: "test".into(),
            description: String::new(),
            enabled: true,
            priority: 10,
            created_order: 1,
            channel: None,
            stage,
            conditions,
            actions,
            one_shot: false,
        }
    }

    fn context<'a>(
        epoch: RuntimeEpoch,
        terminal: &'a TerminalIdentity,
        json: Option<&'a Value>,
    ) -> MatchContext<'a> {
        MatchContext {
            runtime_epoch: epoch,
            channel: ChannelKind::Transaction,
            stage: MessageStage::Request,
            terminal,
            path_or_request_type: Some("/payment"),
            json_body: json,
        }
    }

    // RULE-003, RULE-005, RULE-008, RULE-010, ENGINE-006
    #[test]
    fn evaluates_priority_then_creation_order_and_stops_at_terminal_action() {
        let epoch = RuntimeEpoch::new();
        let mut first = Rule::create(draft(
            MessageStage::Request,
            Vec::new(),
            vec![RuleAction::Delay { milliseconds: 20 }],
        ))
        .unwrap();
        first.priority = 1;
        let mut terminal = Rule::create(draft(
            MessageStage::Request,
            Vec::new(),
            vec![RuleAction::Terminal(
                TerminalAction::DisconnectBeforeUpstream,
            )],
        ))
        .unwrap();
        terminal.priority = 2;
        let mut unreachable = Rule::create(draft(
            MessageStage::Request,
            Vec::new(),
            vec![RuleAction::Delay { milliseconds: 30 }],
        ))
        .unwrap();
        unreachable.priority = 3;
        let terminal_identity = TerminalIdentity {
            source_ip: "10.0.0.1".into(),
            certificate_sha256: "cert".into(),
        };
        let mut engine = RuleEngine::new(epoch, vec![unreachable, terminal, first]);
        let result = engine.evaluate(&context(epoch, &terminal_identity, None), Utc::now());
        assert_eq!(result.composed_actions.len(), 2);
        assert!(result.terminal_action.is_some());
        assert_eq!(result.traces.len(), 2);
    }

    #[test]
    fn equal_priority_and_creation_order_use_rule_id_as_a_stable_tiebreaker() {
        let epoch = RuntimeEpoch::new();
        let first = Rule::create(draft(
            MessageStage::Request,
            Vec::new(),
            vec![RuleAction::Terminal(
                TerminalAction::DisconnectBeforeUpstream,
            )],
        ))
        .expect("first rule");
        let second = Rule::create(draft(
            MessageStage::Request,
            Vec::new(),
            vec![RuleAction::Terminal(
                TerminalAction::DisconnectBeforeUpstream,
            )],
        ))
        .expect("second rule");
        let expected = first.id.min(second.id);
        let terminal = TerminalIdentity {
            source_ip: "10.0.0.1".into(),
            certificate_sha256: "cert".into(),
        };

        for rules in [vec![first.clone(), second.clone()], vec![second, first]] {
            let mut engine = RuleEngine::new(epoch, rules);
            let evaluation = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
            assert_eq!(evaluation.traces[0].rule_id, expected);
        }
    }

    // RULE-004, ENGINE-003, ENGINE-004, TEST-RULE
    #[test]
    fn matches_json_path_equals_contains_and_regex_without_panicking() {
        let epoch = RuntimeEpoch::new();
        let rule = Rule::create(draft(
            MessageStage::Request,
            vec![
                MatchCondition::Field {
                    field: MatchField::JsonPath("$.payment.items[0].name".into()),
                    operator: MatchOperator::Equals("商品A".into()),
                },
                MatchCondition::Field {
                    field: MatchField::PathOrRequestType,
                    operator: MatchOperator::Contains("pay".into()),
                },
                MatchCondition::Field {
                    field: MatchField::TerminalIp,
                    operator: MatchOperator::Regex(r"^10\.0\.".into()),
                },
            ],
            vec![RuleAction::Pause],
        ))
        .unwrap();
        let terminal = TerminalIdentity {
            source_ip: "10.0.0.8".into(),
            certificate_sha256: "cert".into(),
        };
        let body = json!({"payment":{"items":[{"name":"商品A"}]}});
        let mut engine = RuleEngine::new(epoch, vec![rule]);
        assert!(
            engine
                .evaluate(&context(epoch, &terminal, Some(&body)), Utc::now())
                .traces[0]
                .matched
        );
        let no_json = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
        assert!(no_json.traces[0].reason.contains("JSON"));
    }

    #[test]
    fn invalid_persisted_json_path_is_a_non_match_instead_of_a_panic() {
        let epoch = RuntimeEpoch::new();
        let mut rule = Rule::create(draft(
            MessageStage::Request,
            vec![MatchCondition::Field {
                field: MatchField::JsonPath("$.valid".into()),
                operator: MatchOperator::Equals("value".into()),
            }],
            vec![RuleAction::Pause],
        ))
        .expect("initial valid rule");
        rule.conditions = vec![MatchCondition::Field {
            field: MatchField::JsonPath("$.items[]".into()),
            operator: MatchOperator::Equals("value".into()),
        }];
        let terminal = TerminalIdentity {
            source_ip: "10.0.0.8".into(),
            certificate_sha256: "cert".into(),
        };
        let body = json!({"items": ["value"]});
        let mut engine = RuleEngine::new(epoch, vec![rule]);

        let evaluation = engine.evaluate(&context(epoch, &terminal, Some(&body)), Utc::now());

        assert!(!evaluation.traces[0].matched);
        assert!(evaluation.traces[0].reason.contains("未通过保存校验"));
    }

    // RULE-006, RULE-007, ENGINE-007
    #[test]
    fn nth_hit_is_per_terminal_and_resets_on_restart_reenable_and_condition_change() {
        let epoch = RuntimeEpoch::new();
        let mut rule = Rule::create(draft(
            MessageStage::Request,
            vec![MatchCondition::NthHit(2)],
            vec![RuleAction::Pause],
        ))
        .unwrap();
        rule.one_shot = true;
        let id = rule.id;
        let terminal = TerminalIdentity {
            source_ip: "10.0.0.8".into(),
            certificate_sha256: "cert".into(),
        };
        let other_terminal = TerminalIdentity {
            source_ip: "10.0.0.9".into(),
            certificate_sha256: "other-cert".into(),
        };
        let mut engine = RuleEngine::new(epoch, vec![rule]);
        assert!(
            !engine
                .evaluate(&context(epoch, &terminal, None), Utc::now())
                .traces[0]
                .matched
        );
        assert!(
            !engine
                .evaluate(&context(epoch, &other_terminal, None), Utc::now())
                .traces[0]
                .matched
        );
        assert!(
            engine
                .evaluate(&context(epoch, &terminal, None), Utc::now())
                .traces[0]
                .matched
        );
        assert!(!engine.rules()[0].enabled);
        let revision = engine.rules()[0].revision;
        engine.toggle(id, revision, true).unwrap();
        assert!(
            !engine
                .evaluate(&context(epoch, &terminal, None), Utc::now())
                .traces[0]
                .matched
        );
        engine.restart(RuntimeEpoch::new());
        let new_epoch = engine.runtime_epoch.unwrap();
        assert!(
            !engine
                .evaluate(&context(new_epoch, &terminal, None), Utc::now())
                .traces[0]
                .matched
        );
    }

    // RULE-007
    #[test]
    fn changing_match_conditions_resets_existing_hit_counters() {
        let epoch = RuntimeEpoch::new();
        let rule = Rule::create(draft(
            MessageStage::Request,
            vec![MatchCondition::NthHit(2)],
            vec![RuleAction::Pause],
        ))
        .unwrap();
        let id = rule.id;
        let terminal = TerminalIdentity {
            source_ip: "10.0.0.8".into(),
            certificate_sha256: "cert".into(),
        };
        let mut engine = RuleEngine::new(epoch, vec![rule]);
        assert!(
            !engine
                .evaluate(&context(epoch, &terminal, None), Utc::now())
                .traces[0]
                .matched
        );
        let mut changed = draft(
            MessageStage::Request,
            vec![MatchCondition::NthHit(3)],
            vec![RuleAction::Pause],
        );
        changed.expected_revision = Some(Revision::INITIAL);
        engine.save(id, changed).unwrap();
        assert!(
            !engine
                .evaluate(&context(epoch, &terminal, None), Utc::now())
                .traces[0]
                .matched
        );
        assert!(
            !engine
                .evaluate(&context(epoch, &terminal, None), Utc::now())
                .traces[0]
                .matched
        );
    }

    // RULE-007
    #[test]
    fn reconcile_preserves_unrelated_rule_counters_and_resets_changed_rule() {
        let epoch = RuntimeEpoch::new();
        let unchanged = Rule::create(draft(
            MessageStage::Request,
            vec![MatchCondition::NthHit(3)],
            vec![RuleAction::Pause],
        ))
        .unwrap();
        let mut changed = Rule::create(draft(
            MessageStage::Request,
            vec![MatchCondition::NthHit(2)],
            vec![RuleAction::Pause],
        ))
        .unwrap();
        changed.priority = 20;
        let unchanged_id = unchanged.id;
        let changed_id = changed.id;
        let terminal = TerminalIdentity {
            source_ip: "10.0.0.8".into(),
            certificate_sha256: "cert".into(),
        };
        let mut engine = RuleEngine::new(epoch, vec![unchanged.clone(), changed.clone()]);

        let first = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
        assert!(
            first.traces.iter().all(|trace| !trace.matched),
            "both counters should be below their thresholds"
        );

        changed.conditions = vec![MatchCondition::NthHit(3)];
        changed.revision = changed.revision.next();
        engine.reconcile(vec![unchanged, changed]);

        let second = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
        assert!(
            second
                .traces
                .iter()
                .find(|trace| trace.rule_id == unchanged_id)
                .is_some_and(|trace| !trace.matched)
        );
        assert!(
            second
                .traces
                .iter()
                .find(|trace| trace.rule_id == changed_id)
                .is_some_and(|trace| !trace.matched)
        );

        let third = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
        assert!(
            third
                .traces
                .iter()
                .find(|trace| trace.rule_id == unchanged_id)
                .is_some_and(|trace| trace.matched),
            "editing another rule must not reset this rule's Nth-hit counter"
        );
        assert!(
            third
                .traces
                .iter()
                .find(|trace| trace.rule_id == changed_id)
                .is_some_and(|trace| !trace.matched),
            "the changed rule must restart its Nth-hit counter"
        );
    }

    // RULE-002, RULE-007
    #[test]
    fn displayed_hit_metadata_resets_on_restart_and_reenable() {
        let epoch = RuntimeEpoch::new();
        let rule = Rule::create(draft(
            MessageStage::Request,
            Vec::new(),
            vec![RuleAction::Pause],
        ))
        .unwrap();
        let id = rule.id;
        let terminal = TerminalIdentity {
            source_ip: "10.0.0.8".into(),
            certificate_sha256: "cert".into(),
        };
        let mut engine = RuleEngine::new(epoch, vec![rule]);
        engine.evaluate(&context(epoch, &terminal, None), Utc::now());
        assert_eq!(engine.rules()[0].hit_count, 1);
        assert!(engine.rules()[0].last_hit_at.is_some());

        engine.restart(RuntimeEpoch::new());
        assert_eq!(engine.rules()[0].hit_count, 0);
        assert!(engine.rules()[0].last_hit_at.is_none());

        let revision = engine.rules()[0].revision;
        engine.toggle(id, revision, false).unwrap();
        let revision = engine.rules()[0].revision;
        engine.toggle(id, revision, true).unwrap();
        assert_eq!(engine.rules()[0].hit_count, 0);
        assert!(engine.rules()[0].last_hit_at.is_none());
    }

    // ENGINE-005, RULE-011, ACTION-001, ACTION-009
    #[test]
    fn validates_regex_delay_terminal_order_phase_and_action_parameters() {
        let invalid = draft(
            MessageStage::Request,
            vec![MatchCondition::Field {
                field: MatchField::TerminalIp,
                operator: MatchOperator::Regex("(".into()),
            }],
            vec![
                RuleAction::Delay {
                    milliseconds: MAX_TOTAL_DELAY_MS + 1,
                },
                RuleAction::Terminal(TerminalAction::IncorrectContentLength { delta: 0 }),
                RuleAction::Pause,
            ],
        );
        let error = validate_rule_draft(&invalid).unwrap_err();
        assert_eq!(error.code, ErrorCode::RuleInvalid);
        assert!(error.field_errors.len() >= 4);
    }

    // RULE-009~011, ACTION-001~013, TEST-RULE:
    // every UI action has one explicit legal phase contract.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn validates_every_action_against_its_exact_stage_contract() {
        let cases = vec![
            (
                RuleAction::SetJsonField {
                    path: "$.value".into(),
                    value: json!(1),
                },
                vec![MessageStage::Request, MessageStage::Response],
            ),
            (
                RuleAction::ReplaceBodyText("本文".into()),
                vec![MessageStage::Request, MessageStage::Response],
            ),
            (
                RuleAction::SetHeader {
                    name: "x-test".into(),
                    value: "enabled".into(),
                },
                vec![MessageStage::Request, MessageStage::Response],
            ),
            (
                RuleAction::Delay { milliseconds: 1 },
                vec![MessageStage::Request, MessageStage::Response],
            ),
            (
                RuleAction::Pause,
                vec![MessageStage::Request, MessageStage::Response],
            ),
            (
                RuleAction::CustomHttpStatus { status: 503 },
                vec![MessageStage::Response],
            ),
            (
                RuleAction::Terminal(TerminalAction::RejectTlsHandshake),
                vec![MessageStage::TlsHandshake],
            ),
            (
                RuleAction::Terminal(TerminalAction::DisconnectBeforeUpstream),
                vec![MessageStage::Request],
            ),
            (
                RuleAction::Terminal(TerminalAction::UpstreamConnectTimeout { milliseconds: 1 }),
                vec![MessageStage::Request],
            ),
            (
                RuleAction::Terminal(TerminalAction::UpstreamWriteTimeout { milliseconds: 1 }),
                vec![MessageStage::Request],
            ),
            (
                RuleAction::Terminal(TerminalAction::UpstreamReadTimeout { milliseconds: 1 }),
                vec![MessageStage::Request],
            ),
            (
                RuleAction::Terminal(TerminalAction::DropUpstreamResponse {
                    mode: DropResponseMode::ReadCompleteResponse,
                }),
                vec![MessageStage::Request],
            ),
            (
                RuleAction::Terminal(TerminalAction::DropUpstreamResponse {
                    mode: DropResponseMode::CloseAfterRequestWrite,
                }),
                vec![MessageStage::Request],
            ),
            (
                RuleAction::Terminal(TerminalAction::MockResponse {
                    status: 200,
                    headers: vec![("x-mock".into(), "true".into())],
                    shift_jis_body: b"{}".to_vec(),
                }),
                vec![MessageStage::Request],
            ),
            (
                RuleAction::Terminal(TerminalAction::InvalidJson {
                    shift_jis_body: b"{".to_vec(),
                }),
                vec![MessageStage::Response],
            ),
            (
                RuleAction::Terminal(TerminalAction::IncorrectContentLength { delta: 1 }),
                vec![MessageStage::Response],
            ),
            (
                RuleAction::Terminal(TerminalAction::TruncateResponse { bytes: 0 }),
                vec![MessageStage::Response],
            ),
        ];
        let all_stages = [
            MessageStage::TlsHandshake,
            MessageStage::Request,
            MessageStage::Response,
        ];

        for (action, legal_stages) in cases {
            for stage in all_stages {
                let result = validate_rule_draft(&draft(stage, Vec::new(), vec![action.clone()]));
                assert_eq!(
                    result.is_ok(),
                    legal_stages.contains(&stage),
                    "unexpected phase contract for {action:?} at {stage:?}: {result:?}"
                );
            }
        }
    }

    // RULE-006~007, ENGINE-007, TEST-RULE:
    // NthHit identity is exactly the pair (terminal IP, certificate fingerprint).
    #[test]
    fn nth_hit_counter_is_scoped_by_both_terminal_identity_components() {
        let epoch = RuntimeEpoch::new();
        let rule = Rule::create(draft(
            MessageStage::Request,
            vec![MatchCondition::NthHit(2)],
            vec![RuleAction::Pause],
        ))
        .expect("valid nth-hit rule");
        let base = TerminalIdentity {
            source_ip: "10.0.0.8".into(),
            certificate_sha256: "cert-a".into(),
        };
        let same_ip_other_cert = TerminalIdentity {
            source_ip: base.source_ip.clone(),
            certificate_sha256: "cert-b".into(),
        };
        let other_ip_same_cert = TerminalIdentity {
            source_ip: "10.0.0.9".into(),
            certificate_sha256: base.certificate_sha256.clone(),
        };
        let mut engine = RuleEngine::new(epoch, vec![rule]);

        for identity in [&base, &same_ip_other_cert, &other_ip_same_cert] {
            assert!(
                !engine
                    .evaluate(&context(epoch, identity, None), Utc::now())
                    .traces[0]
                    .matched
            );
        }
        for identity in [&base, &same_ip_other_cert, &other_ip_same_cert] {
            assert!(
                engine
                    .evaluate(&context(epoch, identity, None), Utc::now())
                    .traces[0]
                    .matched,
                "each IP + certificate pair must maintain an independent count"
            );
        }
    }

    #[test]
    fn tls_rejection_preserves_nth_hit_semantics() {
        let epoch = RuntimeEpoch::new();
        let rule = Rule::create(draft(
            MessageStage::TlsHandshake,
            vec![MatchCondition::NthHit(2)],
            vec![RuleAction::Terminal(TerminalAction::RejectTlsHandshake)],
        ))
        .expect("TLS NthHit is a valid pre-HTTP condition");
        let identity = TerminalIdentity {
            source_ip: "10.0.34.94".into(),
            certificate_sha256: "terminal-cert".into(),
        };
        let mut engine = RuleEngine::new(epoch, vec![rule]);
        let tls_context = MatchContext {
            runtime_epoch: epoch,
            channel: ChannelKind::Dll,
            stage: MessageStage::TlsHandshake,
            terminal: &identity,
            path_or_request_type: None,
            json_body: None,
        };

        let first = engine.evaluate(&tls_context, Utc::now());
        assert!(!first.traces[0].matched);
        assert!(first.composed_actions.is_empty());
        assert!(first.terminal_action.is_none());

        let second = engine.evaluate(&tls_context, Utc::now());
        assert!(second.traces[0].matched);
        assert!(matches!(
            second.composed_actions.as_slice(),
            [RuleAction::Terminal(TerminalAction::RejectTlsHandshake)]
        ));
        assert!(matches!(
            second.terminal_action,
            Some(TerminalAction::RejectTlsHandshake)
        ));

        let third = engine.evaluate(&tls_context, Utc::now());
        assert!(!third.traces[0].matched);
        assert!(third.composed_actions.is_empty());
        assert!(third.terminal_action.is_none());
    }

    // ACTION-001, ACTION-011
    #[test]
    fn validates_tls_match_scope_and_truncation_boundary() {
        let tls_rule = draft(
            MessageStage::TlsHandshake,
            vec![MatchCondition::Field {
                field: MatchField::TerminalIp,
                operator: MatchOperator::Equals("10.0.0.8".into()),
            }],
            vec![RuleAction::Terminal(TerminalAction::RejectTlsHandshake)],
        );
        assert!(validate_rule_draft(&tls_rule).is_err());

        let truncate = TerminalAction::TruncateResponse { bytes: 2 };
        assert!(truncate.validate_for_body(3).is_ok());
        assert!(truncate.validate_for_body(2).is_err());
        assert!(
            TerminalAction::TruncateResponse { bytes: 0 }
                .validate_for_body(1)
                .is_ok()
        );
        assert!(
            TerminalAction::TruncateResponse { bytes: 0 }
                .validate_for_body(0)
                .is_err()
        );
    }

    #[test]
    fn validates_json_paths_headers_shift_jis_and_json_fault_bodies_before_save() {
        let invalid = draft(
            MessageStage::Response,
            vec![MatchCondition::Field {
                field: MatchField::JsonPath("$.items[]".into()),
                operator: MatchOperator::Equals("x".into()),
            }],
            vec![
                RuleAction::SetJsonField {
                    path: "missing_root.field".into(),
                    value: json!("🧪"),
                },
                RuleAction::ReplaceBodyText("emoji 🧪".into()),
                RuleAction::SetHeader {
                    name: "content-length".into(),
                    value: "12\r\nx-injected: yes".into(),
                },
                RuleAction::Terminal(TerminalAction::MockResponse {
                    status: 200,
                    headers: vec![("bad header".into(), "value".into())],
                    shift_jis_body: vec![0x82],
                }),
            ],
        );
        let error = validate_rule_draft(&invalid).expect_err("all invalid fields fail closed");
        for field in [
            "conditions.0.path",
            "actions.0.path",
            "actions.0.value_json",
            "actions.1.text",
            "actions.2.name",
            "actions.2.value",
            "actions.3.headers.0.name",
            "actions.3.shift_jis_body",
        ] {
            assert!(
                error.field_errors.contains_key(field),
                "missing field error for {field}: {:?}",
                error.field_errors
            );
        }

        let valid_invalid_json = draft(
            MessageStage::Response,
            Vec::new(),
            vec![RuleAction::Terminal(TerminalAction::InvalidJson {
                shift_jis_body: b"{".to_vec(),
            })],
        );
        assert!(validate_rule_draft(&valid_invalid_json).is_ok());

        let accidentally_valid_json = draft(
            MessageStage::Response,
            Vec::new(),
            vec![RuleAction::Terminal(TerminalAction::InvalidJson {
                shift_jis_body: b"{}".to_vec(),
            })],
        );
        assert!(validate_rule_draft(&accidentally_valid_json).is_err());
    }

    // RULE-012
    #[test]
    fn warns_when_higher_priority_terminal_rule_can_shadow_lower_rule() {
        let epoch = RuntimeEpoch::new();
        let higher = Rule::create(draft(
            MessageStage::Request,
            Vec::new(),
            vec![RuleAction::Terminal(
                TerminalAction::DisconnectBeforeUpstream,
            )],
        ))
        .unwrap();
        let mut lower = Rule::create(draft(
            MessageStage::Request,
            vec![MatchCondition::NthHit(2)],
            vec![RuleAction::Pause],
        ))
        .unwrap();
        lower.priority = 20;
        let warnings = RuleEngine::new(epoch, vec![lower, higher]).conflict_warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, ErrorCode::RuleConflictWarning);
    }

    #[test]
    fn validates_weak_network_parameter_boundaries_and_stages() {
        let valid = draft(
            MessageStage::Request,
            Vec::new(),
            vec![
                RuleAction::Jitter {
                    minimum_milliseconds: 10,
                    maximum_milliseconds: 20,
                    scope: JitterScope::PerChunk,
                },
                RuleAction::Throttle {
                    bytes_per_second: 1,
                    chunk_bytes: MAX_TRAFFIC_CHUNK_BYTES,
                    direction: TrafficDirection::Upstream,
                },
                RuleAction::Intermittent {
                    available_milliseconds: 1,
                    blocked_milliseconds: MAX_TOTAL_DELAY_MS,
                    direction: TrafficDirection::Upstream,
                },
                RuleAction::Terminal(TerminalAction::DisconnectDuringUpstreamWrite {
                    after_bytes: 1,
                }),
            ],
        );
        assert!(validate_rule_draft(&valid).is_ok());

        let invalid = draft(
            MessageStage::Request,
            Vec::new(),
            vec![
                RuleAction::Jitter {
                    minimum_milliseconds: 2,
                    maximum_milliseconds: 1,
                    scope: JitterScope::BeforeMessage,
                },
                RuleAction::Throttle {
                    bytes_per_second: 0,
                    chunk_bytes: MAX_TRAFFIC_CHUNK_BYTES + 1,
                    direction: TrafficDirection::Upstream,
                },
                RuleAction::Intermittent {
                    available_milliseconds: 0,
                    blocked_milliseconds: MAX_TOTAL_DELAY_MS + 1,
                    direction: TrafficDirection::Upstream,
                },
                RuleAction::Terminal(TerminalAction::DisconnectDuringDownstreamWrite {
                    after_bytes: 1,
                }),
            ],
        );
        let error = validate_rule_draft(&invalid).expect_err("weak network bounds");
        for field in [
            "actions.0.minimum_milliseconds",
            "actions.1.bytes_per_second",
            "actions.1.chunk_bytes",
            "actions.2.available_milliseconds",
            "actions.2.blocked_milliseconds",
            "actions.3",
        ] {
            assert!(
                error.field_errors.contains_key(field),
                "missing field error for {field}: {:?}",
                error.field_errors
            );
        }
    }

    #[test]
    fn mid_body_disconnect_offset_is_validated_against_runtime_body() {
        let upstream = TerminalAction::DisconnectDuringUpstreamWrite { after_bytes: 3 };
        assert!(upstream.validate_for_body(4).is_ok());
        assert!(upstream.validate_for_body(3).is_err());
        let downstream = TerminalAction::DisconnectDuringDownstreamWrite { after_bytes: 0 };
        assert!(downstream.validate_for_body(1).is_ok());
        assert!(downstream.validate_for_body(0).is_err());
    }
}
