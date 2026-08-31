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
    pub document_fields: Vec<ProtocolRuleFieldCapability>,
    pub document_common_actions: Vec<ProtocolRuleCommonActionCapability>,
    pub new_rule_draft: RuleDefinitionSaveInput,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuleEditorStage {
    pub stage: RuleStage,
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
    pub local_document_types: Vec<RuleLocalDocumentTypeCapability>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleLocalDocumentValueType {
    String,
    Number,
    Boolean,
    Null,
    Object,
    Array,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleLocalDocumentPredicateKind {
    Equals,
    Contains,
    StartsWith,
    EndsWith,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleLocalDocumentActionKind {
    Set,
    Clear,
    Insert,
    Append,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleLocalDocumentTypeCapability {
    pub value_type: RuleLocalDocumentValueType,
    pub predicates: Vec<RuleLocalDocumentPredicateKind>,
    pub actions: Vec<RuleLocalDocumentActionKind>,
}

#[must_use]
pub fn local_document_type_capabilities() -> Vec<RuleLocalDocumentTypeCapability> {
    use RuleLocalDocumentActionKind::{Append, Clear, Insert, Set};
    use RuleLocalDocumentPredicateKind::{
        Contains, EndsWith, Equals, Greater, GreaterEqual, Less, LessEqual, StartsWith,
    };
    use RuleLocalDocumentValueType::{Array, Boolean, Null, Number, Object, String};
    vec![
        RuleLocalDocumentTypeCapability {
            value_type: String,
            predicates: vec![Equals, Contains, StartsWith, EndsWith],
            actions: vec![Set, Clear, Insert, Append],
        },
        RuleLocalDocumentTypeCapability {
            value_type: Number,
            predicates: vec![Equals, Less, LessEqual, Greater, GreaterEqual],
            actions: vec![Set, Clear, Insert, Append],
        },
        RuleLocalDocumentTypeCapability {
            value_type: Boolean,
            predicates: vec![Equals],
            actions: vec![Set, Clear, Insert, Append],
        },
        RuleLocalDocumentTypeCapability {
            value_type: Null,
            predicates: vec![Equals],
            actions: vec![Set, Clear, Insert, Append],
        },
        RuleLocalDocumentTypeCapability {
            value_type: Object,
            predicates: vec![],
            actions: vec![Set, Clear, Insert, Append],
        },
        RuleLocalDocumentTypeCapability {
            value_type: Array,
            predicates: vec![],
            actions: vec![Set, Clear, Insert, Append],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::local_document_type_capabilities;

    #[test]
    fn schema_free_capabilities_cover_every_document_value_type() {
        let capabilities = local_document_type_capabilities();
        let json = serde_json::to_value(capabilities).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 6);
        assert_eq!(json[0]["value_type"], "string");
        assert_eq!(json[1]["value_type"], "number");
        assert_eq!(json[2]["value_type"], "boolean");
        assert_eq!(json[3]["value_type"], "null");
        assert_eq!(json[4]["value_type"], "object");
        assert_eq!(json[5]["value_type"], "array");
        assert_eq!(json[4]["predicates"], serde_json::json!([]));
        assert_eq!(json[5]["predicates"], serde_json::json!([]));
    }
}
