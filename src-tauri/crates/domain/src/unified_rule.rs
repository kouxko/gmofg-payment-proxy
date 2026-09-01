//! HTTP 与 Socket 共用的规则聚合与阶段坐标。
//!
//! 顶层生命周期、排序、Listener 绑定和 revision 只在 [`RuleDefinition`] 中存在一次；
//! HTTP 与 Socket 的差异能力由带标签的 [`RuleContent`] 保持类型隔离。

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    Condition, DomainError, ErrorCode, ListenerId, ProtocolDirection, ProtocolPackageRef, Revision,
    RuleId, RuleProgramEntry, UnifiedAction,
};

mod lifecycle;
mod validation;

use validation::{ensure_socket_only, validate_http_runtime_content};

pub const MAX_RULE_DEFINITIONS: usize = 1_024;
pub const MAX_DOCUMENT_RULE_STRING_BYTES: usize = 16 * 1_024;

pub use lifecycle::{
    NthCounterAdvance, NthCounterSnapshot, RuleDefinitionRestoreSnapshot, RuleLifecycle,
    RuleLifecycleDelta, RuleLifecycleSnapshot,
};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type,
)]
#[serde(rename_all = "snake_case")]
pub enum RuleStage {
    ProxyToUpstream,
    ProxyToApp,
}

impl RuleStage {
    #[must_use]
    pub const fn direction(self) -> ProtocolDirection {
        match self {
            Self::ProxyToUpstream => ProtocolDirection::Upstream,
            Self::ProxyToApp => ProtocolDirection::Downstream,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct HttpRuleContent {
    pub description: String,
    pub conditions: Vec<Condition>,
    pub actions: Vec<UnifiedAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuleContent {
    pub package: ProtocolPackageRef,
    pub conditions: Vec<Condition>,
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
            (Self::Http(_), Self::Http(_)) => true,
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
    pub one_shot: bool,
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
    one_shot: bool,
    lifecycle: RuleLifecycle,
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
    one_shot: bool,
    lifecycle: RuleLifecycle,
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
            one_shot: value.one_shot,
            lifecycle: value.lifecycle,
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
            one_shot: draft.one_shot,
            lifecycle: RuleLifecycle::default(),
            content: draft.content,
        };
        definition.validate_for_save()?;
        Ok(definition)
    }

    pub fn restore(
        rule_id: RuleId,
        draft: RuleDefinitionDraft,
        snapshot: RuleDefinitionRestoreSnapshot,
    ) -> Result<Self, DomainError> {
        let definition = Self {
            rule_id,
            revision: snapshot.revision,
            name: draft.name,
            enabled: draft.enabled,
            priority: draft.priority,
            created_order: snapshot.created_order,
            listener_id: draft.listener_id,
            stage: draft.stage,
            one_shot: draft.one_shot,
            lifecycle: snapshot.lifecycle,
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
        candidate.one_shot = draft.one_shot;
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
        self.revision = Revision::INITIAL;
        self.lifecycle = RuleLifecycle::default();
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
                validate_http_runtime_content(self, content)?;
                RuleProgramEntry::new(
                    self.rule_id,
                    self.priority,
                    self.created_order,
                    content.conditions.clone(),
                    content.actions.clone(),
                )?;
            }
            RuleContent::Socket(content) => {
                ensure_socket_only(&content.conditions, &content.actions)?;
                RuleProgramEntry::new(
                    self.rule_id,
                    self.priority,
                    self.created_order,
                    content.conditions.clone(),
                    content.actions.clone(),
                )?;
            }
        }
        Ok(())
    }

    /// Validates the persisted rule shape accepted by current create/update operations.
    ///
    /// [`Self::restore`] uses the same current stage enum, so removed legacy stages fail before
    /// this save boundary. Callers projecting a new save from an existing identity still invoke
    /// this validation before persistence.
    pub fn validate_for_save(&self) -> Result<(), DomainError> {
        self.validate()
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
    pub const fn one_shot(&self) -> bool {
        self.one_shot
    }

    #[must_use]
    pub fn to_draft(&self) -> RuleDefinitionDraft {
        RuleDefinitionDraft {
            name: self.name.clone(),
            enabled: self.enabled,
            priority: self.priority,
            listener_id: self.listener_id,
            stage: self.stage,
            one_shot: self.one_shot,
            content: self.content.clone(),
        }
    }
}

fn rule_binding_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "统一规则配置无效").with_field_error(field, message)
}

pub fn sort_rule_definitions(rules: &mut [RuleDefinition]) {
    rules.sort_by_key(|rule| (rule.priority(), rule.rule_id()));
}
