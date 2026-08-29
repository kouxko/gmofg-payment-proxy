//! HTTP 与 Socket 共用的规则聚合与阶段坐标。
//!
//! 顶层生命周期、排序、Listener 绑定和 revision 只在 [`RuleDefinition`] 中存在一次；
//! HTTP 与 Socket 的差异能力由带标签的 [`RuleContent`] 保持类型隔离。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    DocumentAction, DocumentCondition, DomainError, ErrorCode, ListenerId, MatchCondition,
    MessageStage, ProtocolDirection, ProtocolPackageRef, Revision, RuleAction, RuleDraft, RuleId,
    validate_document_rule_content_structure, validate_rule_draft,
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
    pub conditions: Vec<DocumentCondition>,
    pub actions: Vec<DocumentAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct HttpRuleContent {
    pub description: String,
    pub conditions: Vec<MatchCondition>,
    pub actions: Vec<RuleAction>,
    pub document: Option<HttpDocumentRuleContent>,
    pub one_shot: bool,
    pub hit_count: u64,
    pub last_hit_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuleContent {
    pub package: ProtocolPackageRef,
    pub conditions: Vec<DocumentCondition>,
    pub actions: Vec<DocumentAction>,
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
        definition.validate()?;
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
        candidate.validate()?;
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
                validate_http_runtime_content(self, content)?;
                if let Some(document) = &content.document {
                    validate_document_rule_content_structure(
                        &document.conditions,
                        &document.actions,
                    )?;
                }
            }
            RuleContent::Socket(content) => {
                if self.stage == RuleStage::TlsHandshake {
                    return Err(rule_binding_error(
                        "stage",
                        "Socket 消息规则不能使用 TLS 握手阶段",
                    ));
                }
                validate_document_rule_content_structure(
                    &content.conditions,
                    content.actions.as_slice(),
                )?;
            }
        }
        Ok(())
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
    let has_ordinary_http_work = !content.conditions.is_empty() || !content.actions.is_empty();
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
        conditions: content.conditions.clone(),
        actions: content.actions.clone(),
        one_shot: content.one_shot,
    })
}

fn rule_binding_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "统一规则配置无效").with_field_error(field, message)
}

pub fn sort_rule_definitions(rules: &mut [RuleDefinition]) {
    rules.sort_by_key(|rule| {
        (
            rule.stage(),
            rule.priority(),
            rule.created_order(),
            rule.rule_id(),
        )
    });
}
