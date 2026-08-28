use serde::{Deserialize, Serialize};
use specta::Type;

use intercept_proxy_domain::{Revision, RuleDefinitionDraft, RuleId, RuleStage};

use super::{
    ListenerId, ProtocolPackageRef, ProtocolRuleCommonActionCapability,
    ProtocolRuleFieldCapability, RuleStageCapabilityViewModel,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleDefinitionSaveInput {
    pub rule_id: Option<RuleId>,
    pub expected_revision: Option<Revision>,
    pub draft: RuleDefinitionDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct HttpRuleEditorStage {
    pub stage: RuleStage,
    pub http: Option<RuleStageCapabilityViewModel>,
    pub package: Option<ProtocolPackageRef>,
    pub schema_version: Option<u32>,
    pub document_fields: Vec<ProtocolRuleFieldCapability>,
    pub document_common_actions: Vec<ProtocolRuleCommonActionCapability>,
    pub new_rule_draft: RuleDefinitionSaveInput,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuleEditorStage {
    pub stage: RuleStage,
    pub schema_version: u32,
    pub fields: Vec<ProtocolRuleFieldCapability>,
    pub common_actions: Vec<ProtocolRuleCommonActionCapability>,
    pub new_rule_draft: RuleDefinitionSaveInput,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RuleEditorContentContext {
    Http {
        stages: Vec<HttpRuleEditorStage>,
    },
    Socket {
        package: ProtocolPackageRef,
        stages: Vec<SocketRuleEditorStage>,
    },
}

/// Rust-authoritative editor contract for one Listener.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleEditorContext {
    pub listener_id: ListenerId,
    pub content: RuleEditorContentContext,
}
