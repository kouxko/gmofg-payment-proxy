//! HTTP 与 Socket 共用的规则聚合与阶段坐标。
//!
//! 顶层生命周期、排序、Listener 绑定和 revision 只在 [`RuleDefinition`] 中存在一次；
//! HTTP 与 Socket 的差异能力由带标签的 [`RuleContent`] 保持类型隔离。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Condition, ConditionTree, DomainError, ErrorCode, ListenerId, MatchCondition, MessageStage,
    ProtocolDirection, ProtocolPackageRef, Revision, RuleAction, RuleDraft, RuleId,
    RuleProgramEntry, UnifiedAction, validate_rule_draft,
};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum RuleStage {
    AppToProxy,
    ProxyToUpstream,
    UpstreamToProxy,
    ProxyToApp,
    TlsHandshake,
}

impl RuleStage {
    #[must_use]
    pub const fn direction(self) -> Option<ProtocolDirection> {
        match self {
            Self::AppToProxy | Self::ProxyToUpstream => Some(ProtocolDirection::Upstream),
            Self::UpstreamToProxy | Self::ProxyToApp => Some(ProtocolDirection::Downstream),
            Self::TlsHandshake => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct HttpDocumentRuleContent {
    pub package: ProtocolPackageRef,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct HttpRuleContent {
    pub description: String,
    pub condition: ConditionTree,
    pub actions: Vec<UnifiedAction>,
    pub document: Option<HttpDocumentRuleContent>,
    pub one_shot: bool,
    pub hit_count: u64,
    pub last_hit_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuleContent {
    pub package: ProtocolPackageRef,
    pub condition: ConditionTree,
    pub actions: Vec<UnifiedAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RuleContent {
    Http(HttpRuleContent),
    Socket(SocketRuleContent),
}

impl RuleContent {
    const fn kind(&self) -> RuleContentKind {
        match self {
            Self::Http(_) => RuleContentKind::Http,
            Self::Socket(_) => RuleContentKind::Socket,
        }
    }

    fn immutable_binding_matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Http(current), Self::Http(candidate)) => {
                match (&current.document, &candidate.document) {
                    (None, None) => true,
                    (Some(current), Some(candidate)) => current.package == candidate.package,
                    _ => false,
                }
            }
            (Self::Socket(current), Self::Socket(candidate)) => {
                current.package == candidate.package
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuleContentKind {
    Http,
    Socket,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleDefinitionDraft {
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub listener_id: ListenerId,
    pub stage: RuleStage,
    pub content: RuleContent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(try_from = "RuleDefinitionWire")]
pub struct RuleDefinition {
    rule_id: RuleId,
    revision: Revision,
    name: String,
    enabled: bool,
    priority: i32,
    created_order: u64,
    listener_id: ListenerId,
    stage: RuleStage,
    content: RuleContent,
}

#[derive(Deserialize, Type)]
#[serde(deny_unknown_fields)]
struct RuleDefinitionWire {
    rule_id: RuleId,
    revision: Revision,
    name: String,
    enabled: bool,
    priority: i32,
    created_order: u64,
    listener_id: ListenerId,
    stage: RuleStage,
    content: RuleContent,
}

impl TryFrom<RuleDefinitionWire> for RuleDefinition {
    type Error = DomainError;

    fn try_from(value: RuleDefinitionWire) -> Result<Self, Self::Error> {
        let definition = Self {
            rule_id: value.rule_id,
            revision: value.revision,
            name: value.name,
            enabled: value.enabled,
            priority: value.priority,
            created_order: value.created_order,
            listener_id: value.listener_id,
            stage: value.stage,
            content: value.content,
        };
        definition.validate()?;
        Ok(definition)
    }
}

impl RuleDefinition {
    pub fn create(draft: RuleDefinitionDraft, created_order: u64) -> Result<Self, DomainError> {
        let definition = Self {
            rule_id: RuleId::new(),
            revision: Revision::INITIAL,
            name: draft.name,
            enabled: draft.enabled,
            priority: draft.priority,
            created_order,
            listener_id: draft.listener_id,
            stage: draft.stage,
            content: draft.content,
        };
        definition.validate_for_save()?;
        Ok(definition)
    }

    pub fn restore(
        rule_id: RuleId,
        revision: Revision,
        draft: RuleDefinitionDraft,
        created_order: u64,
    ) -> Result<Self, DomainError> {
        let definition = Self {
            rule_id,
            revision,
            name: draft.name,
            enabled: draft.enabled,
            priority: draft.priority,
            created_order,
            listener_id: draft.listener_id,
            stage: draft.stage,
            content: draft.content,
        };
        definition.validate()?;
        Ok(definition)
    }

    pub fn update(
        &mut self,
        expected_revision: Revision,
        draft: RuleDefinitionDraft,
    ) -> Result<Revision, DomainError> {
        self.revision.verify(expected_revision)?;
        if self.listener_id != draft.listener_id {
            return Err(rule_binding_error(
                "listener_id",
                "规则创建后不能切换 Listener；请创建新规则",
            ));
        }
        if self.content.kind() != draft.content.kind()
            || !self.content.immutable_binding_matches(&draft.content)
        {
            return Err(rule_binding_error(
                "content",
                "规则创建后不能切换内容类型、协议包或 Schema",
            ));
        }
        let mut candidate = self.clone();
        candidate.name = draft.name;
        candidate.enabled = draft.enabled;
        candidate.priority = draft.priority;
        candidate.stage = draft.stage;
        candidate.content = draft.content;
        candidate.validate_for_save()?;
        candidate.revision = self.revision.checked_next()?;
        let revision = candidate.revision;
        *self = candidate;
        Ok(revision)
    }

    pub fn set_enabled(
        &mut self,
        expected_revision: Revision,
        enabled: bool,
    ) -> Result<Revision, DomainError> {
        self.revision.verify(expected_revision)?;
        self.revision = self.revision.checked_next()?;
        self.enabled = enabled;
        Ok(self.revision)
    }

    pub fn remap_for_workspace_copy(&mut self, listener_id: ListenerId) {
        self.rule_id = RuleId::new();
        self.listener_id = listener_id;
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        if self.name.trim().is_empty() {
            return Err(rule_binding_error("name", "规则名称不能为空"));
        }
        if self.revision.get() == 0 {
            return Err(rule_binding_error("revision", "规则 revision 必须为正整数"));
        }
        if self.created_order == 0 {
            return Err(rule_binding_error(
                "created_order",
                "规则创建顺序必须为正整数",
            ));
        }
        match &self.content {
            RuleContent::Http(content) => {
                if self.stage == RuleStage::TlsHandshake && content.document.is_some() {
                    return Err(rule_binding_error(
                        "content.document",
                        "TLS 握手阶段不能执行 HTTP Body Document 规则",
                    ));
                }
                if content.document.is_none()
                    && (content.condition.contains_document_condition()
                        || content
                            .actions
                            .iter()
                            .any(|action| matches!(action, UnifiedAction::Document(_))))
                {
                    return Err(rule_binding_error(
                        "content.document",
                        "HTTP 规则未绑定 Document 软件包时不能包含 Document 条件或动作",
                    ));
                }
                validate_http_runtime_content(self, content)?;
                RuleProgramEntry::new(
                    self.rule_id,
                    self.priority,
                    self.created_order,
                    content.condition.clone(),
                    content.actions.clone(),
                )?;
            }
            RuleContent::Socket(content) => {
                if self.stage == RuleStage::TlsHandshake {
                    return Err(rule_binding_error(
                        "stage",
                        "Socket 消息规则不能使用 TLS 握手阶段",
                    ));
                }
                ensure_socket_only(&content.condition, &content.actions)?;
                RuleProgramEntry::new(
                    self.rule_id,
                    self.priority,
                    self.created_order,
                    content.condition.clone(),
                    content.actions.clone(),
                )?;
            }
        }
        Ok(())
    }

    /// Validates the persisted rule shape accepted by current create/update operations.
    ///
    /// [`Self::restore`] intentionally accepts legacy message stages until Phase 12 so old
    /// records remain readable. Callers projecting a new save from an existing identity must
    /// invoke this stricter boundary before persistence.
    pub fn validate_for_save(&self) -> Result<(), DomainError> {
        self.validate()?;
        self.validate_new_save_stage()
    }

    fn validate_new_save_stage(&self) -> Result<(), DomainError> {
        match (&self.content, self.stage) {
            (RuleContent::Http(_), RuleStage::TlsHandshake)
            | (
                RuleContent::Http(_) | RuleContent::Socket(_),
                RuleStage::ProxyToUpstream | RuleStage::ProxyToApp,
            ) => Ok(()),
            (
                RuleContent::Http(_) | RuleContent::Socket(_),
                RuleStage::AppToProxy | RuleStage::UpstreamToProxy,
            ) => Err(rule_binding_error(
                "stage",
                "新消息规则只允许 proxy_to_upstream 或 proxy_to_app；旧四阶段数据仅保留到 Phase 12 运行时删除",
            )),
            (RuleContent::Socket(_), RuleStage::TlsHandshake) => Err(rule_binding_error(
                "stage",
                "Socket 消息规则不能使用 TLS 握手阶段",
            )),
        }
    }

    #[must_use]
    pub const fn rule_id(&self) -> RuleId {
        self.rule_id
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn priority(&self) -> i32 {
        self.priority
    }

    #[must_use]
    pub const fn created_order(&self) -> u64 {
        self.created_order
    }

    #[must_use]
    pub const fn listener_id(&self) -> ListenerId {
        self.listener_id
    }

    #[must_use]
    pub const fn stage(&self) -> RuleStage {
        self.stage
    }

    #[must_use]
    pub const fn content(&self) -> &RuleContent {
        &self.content
    }

    #[must_use]
    pub fn to_draft(&self) -> RuleDefinitionDraft {
        RuleDefinitionDraft {
            name: self.name.clone(),
            enabled: self.enabled,
            priority: self.priority,
            listener_id: self.listener_id,
            stage: self.stage,
            content: self.content.clone(),
        }
    }
}

fn validate_http_runtime_content(
    definition: &RuleDefinition,
    content: &HttpRuleContent,
) -> Result<(), DomainError> {
    let mut conditions = Vec::new();
    collect_http_conditions(&content.condition, &mut conditions);
    let actions = content
        .actions
        .iter()
        .filter_map(|action| match action {
            UnifiedAction::Http(action) => Some(action.clone()),
            UnifiedAction::Terminal(action) => Some(RuleAction::Terminal(action.clone())),
            UnifiedAction::RecordMatch | UnifiedAction::Document(_) => None,
        })
        .collect::<Vec<_>>();
    let has_ordinary_http_work = !conditions.is_empty() || !actions.is_empty();
    if !has_ordinary_http_work && content.document.is_some() {
        return Ok(());
    }
    let stage = match definition.stage {
        RuleStage::ProxyToUpstream => MessageStage::Request,
        RuleStage::ProxyToApp => MessageStage::Response,
        RuleStage::TlsHandshake => MessageStage::TlsHandshake,
        RuleStage::AppToProxy | RuleStage::UpstreamToProxy => {
            return Err(rule_binding_error(
                "stage",
                "该处理阶段只支持 Document 条件和动作，不支持普通 HTTP 条件或动作",
            ));
        }
    };
    let priority = u32::try_from(definition.priority)
        .map_err(|_| rule_binding_error("priority", "HTTP 规则优先级不能为负数"))?;
    validate_rule_draft(&RuleDraft {
        expected_revision: Some(definition.revision),
        name: definition.name.clone(),
        description: content.description.clone(),
        enabled: definition.enabled,
        priority,
        created_order: definition.created_order,
        channel: None,
        stage,
        conditions,
        actions,
        one_shot: content.one_shot,
    })
}

fn collect_http_conditions(tree: &ConditionTree, output: &mut Vec<MatchCondition>) {
    match tree {
        ConditionTree::All(children) | ConditionTree::Any(children) => {
            for child in children {
                collect_http_conditions(child, output);
            }
        }
        ConditionTree::Leaf(Condition::Http { condition }) => output.push(condition.clone()),
        ConditionTree::Leaf(Condition::Document { .. }) => {}
    }
}

fn ensure_socket_only(tree: &ConditionTree, actions: &[UnifiedAction]) -> Result<(), DomainError> {
    let mut http = Vec::new();
    collect_http_conditions(tree, &mut http);
    if !http.is_empty()
        || actions
            .iter()
            .any(|action| matches!(action, UnifiedAction::Http(_) | UnifiedAction::Terminal(_)))
    {
        return Err(rule_binding_error(
            "content",
            "Socket 规则不能包含 HTTP 条件、HTTP 动作或尚未声明的终止动作",
        ));
    }
    Ok(())
}

fn rule_binding_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "统一规则配置无效").with_field_error(field, message)
}

pub fn sort_rule_definitions(rules: &mut [RuleDefinition]) {
    rules.sort_by_key(|rule| (rule.priority(), rule.rule_id()));
}
