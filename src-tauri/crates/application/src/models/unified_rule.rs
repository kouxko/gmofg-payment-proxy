use serde::{Deserialize, Serialize};
use specta::Type;

use intercept_proxy_domain::{
    DocumentSchemaNode, Revision, RuleContent, RuleDefinitionDraft, RuleId, RuleStage,
};

use super::{ListenerId, ProtocolPackageRef, RuleActionKind, RuleStageCapabilityViewModel};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleCommonActionCapability {
    RecordMatch,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleDefinitionSaveInput {
    pub rule_id: Option<RuleId>,
    pub expected_revision: Option<Revision>,
    pub draft: RuleDefinitionDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct HttpRuleEditorStageViewModel {
    pub stage: RuleStage,
    pub http: Option<RuleStageCapabilityViewModel>,
    pub package: Option<ProtocolPackageRef>,
    pub document_fields: Vec<RuleDocumentSchemaFieldCapability>,
    pub document_common_actions: Vec<RuleCommonActionCapability>,
    pub new_rule_draft: RuleNewDefinitionDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuleEditorStageViewModel {
    pub stage: RuleStage,
    pub document_fields: Vec<RuleDocumentSchemaFieldCapability>,
    pub common_actions: Vec<RuleCommonActionCapability>,
    pub new_rule_draft: RuleNewDefinitionDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleNewDefinitionDraft {
    pub listener_id: ListenerId,
    pub stage: RuleStage,
    pub content: RuleContent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum RuleEditorContentContext {
    Http {
        stages: Vec<HttpRuleEditorStageViewModel>,
    },
    Socket {
        package: ProtocolPackageRef,
        stages: Vec<SocketRuleEditorStageViewModel>,
    },
}

/// Rust-authoritative editor contract for one Listener.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleEditorContext {
    pub listener_id: ListenerId,
    pub content: RuleEditorContentContext,
    pub local_document_types: Vec<RuleLocalDocumentTypeCapability>,
    pub document_condition_path: RuleDocumentConditionPathCapability,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleDocumentConditionPathCapability {
    pub wildcard_token: String,
    pub wildcard_matches_exactly_one_level: bool,
    pub multiple_matches_use_any: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleNthHitConditionDraftInput {
    pub count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleHttpActionDraftInput {
    pub kind: RuleActionKind,
    pub parameters_json: Option<String>,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleDocumentActionTargetKind {
    Node,
    Array,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleDocumentActionCapability {
    pub kind: RuleLocalDocumentActionKind,
    pub target_kind: RuleDocumentActionTargetKind,
    pub target_value_type: RuleLocalDocumentValueType,
    pub operand_value_type: Option<RuleLocalDocumentValueType>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleDocumentSchemaFieldCapability {
    pub path: String,
    pub label: String,
    pub value_type: RuleLocalDocumentValueType,
    pub item_template: bool,
    pub predicates: Vec<RuleLocalDocumentPredicateKind>,
    pub actions: Vec<RuleDocumentActionCapability>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleLocalDocumentTypeCapability {
    pub value_type: RuleLocalDocumentValueType,
    pub predicates: Vec<RuleLocalDocumentPredicateKind>,
    pub actions: Vec<RuleDocumentActionCapability>,
}

#[must_use]
pub fn local_document_type_capabilities() -> Vec<RuleLocalDocumentTypeCapability> {
    use RuleLocalDocumentPredicateKind::{
        Contains, EndsWith, Equals, Greater, GreaterEqual, Less, LessEqual, StartsWith,
    };
    use RuleLocalDocumentValueType::{Array, Boolean, Null, Number, Object, String};
    vec![
        RuleLocalDocumentTypeCapability {
            value_type: String,
            predicates: vec![Equals, Contains, StartsWith, EndsWith],
            actions: schema_free_actions(String),
        },
        RuleLocalDocumentTypeCapability {
            value_type: Number,
            predicates: vec![Equals, Less, LessEqual, Greater, GreaterEqual],
            actions: schema_free_actions(Number),
        },
        RuleLocalDocumentTypeCapability {
            value_type: Boolean,
            predicates: vec![Equals],
            actions: schema_free_actions(Boolean),
        },
        RuleLocalDocumentTypeCapability {
            value_type: Null,
            predicates: vec![Equals],
            actions: schema_free_actions(Null),
        },
        RuleLocalDocumentTypeCapability {
            value_type: Object,
            predicates: vec![],
            actions: schema_free_actions(Object),
        },
        RuleLocalDocumentTypeCapability {
            value_type: Array,
            predicates: vec![],
            actions: schema_free_actions(Array),
        },
    ]
}

fn schema_free_actions(
    value_type: RuleLocalDocumentValueType,
) -> Vec<RuleDocumentActionCapability> {
    use RuleDocumentActionTargetKind::{Array, Node};
    use RuleLocalDocumentActionKind::{Append, Clear, Insert, Set};
    vec![
        RuleDocumentActionCapability {
            kind: Set,
            target_kind: Node,
            target_value_type: value_type,
            operand_value_type: Some(value_type),
        },
        RuleDocumentActionCapability {
            kind: Clear,
            target_kind: Node,
            target_value_type: value_type,
            operand_value_type: None,
        },
        RuleDocumentActionCapability {
            kind: Insert,
            target_kind: Array,
            target_value_type: RuleLocalDocumentValueType::Array,
            operand_value_type: Some(value_type),
        },
        RuleDocumentActionCapability {
            kind: Append,
            target_kind: Array,
            target_value_type: RuleLocalDocumentValueType::Array,
            operand_value_type: Some(value_type),
        },
    ]
}

#[must_use]
pub fn document_schema_field_capabilities(
    schema: &DocumentSchemaNode,
) -> Vec<RuleDocumentSchemaFieldCapability> {
    let mut fields = Vec::new();
    visit_document_schema(schema, "", false, &mut fields);
    fields
}

fn visit_document_schema(
    node: &DocumentSchemaNode,
    path: &str,
    item_template: bool,
    fields: &mut Vec<RuleDocumentSchemaFieldCapability>,
) {
    let value_type = schema_value_type(node);
    fields.push(RuleDocumentSchemaFieldCapability {
        path: path.to_owned(),
        label: node
            .title()
            .unwrap_or(if path.is_empty() { "/" } else { path })
            .to_owned(),
        value_type,
        item_template,
        predicates: schema_predicates(value_type),
        actions: if item_template {
            Vec::new()
        } else {
            schema_actions(node, value_type)
        },
    });
    match node {
        DocumentSchemaNode::Object { properties, .. } => {
            for (name, child) in properties {
                visit_document_schema(
                    child,
                    &format!("{path}/{}", escape_pointer_token(name)),
                    item_template,
                    fields,
                );
            }
        }
        DocumentSchemaNode::Array { items, .. } => {
            visit_document_schema(items, &format!("{path}/*"), true, fields);
        }
        DocumentSchemaNode::String { .. }
        | DocumentSchemaNode::Number { .. }
        | DocumentSchemaNode::Boolean { .. } => {}
    }
}

const fn schema_value_type(node: &DocumentSchemaNode) -> RuleLocalDocumentValueType {
    match node {
        DocumentSchemaNode::String { .. } => RuleLocalDocumentValueType::String,
        DocumentSchemaNode::Number { .. } => RuleLocalDocumentValueType::Number,
        DocumentSchemaNode::Boolean { .. } => RuleLocalDocumentValueType::Boolean,
        DocumentSchemaNode::Object { .. } => RuleLocalDocumentValueType::Object,
        DocumentSchemaNode::Array { .. } => RuleLocalDocumentValueType::Array,
    }
}

fn schema_predicates(
    value_type: RuleLocalDocumentValueType,
) -> Vec<RuleLocalDocumentPredicateKind> {
    use RuleLocalDocumentPredicateKind::{
        Contains, EndsWith, Equals, Greater, GreaterEqual, Less, LessEqual, StartsWith,
    };
    match value_type {
        RuleLocalDocumentValueType::String => vec![Equals, Contains, StartsWith, EndsWith],
        RuleLocalDocumentValueType::Number => {
            vec![Equals, Less, LessEqual, Greater, GreaterEqual]
        }
        RuleLocalDocumentValueType::Boolean | RuleLocalDocumentValueType::Null => vec![Equals],
        RuleLocalDocumentValueType::Object | RuleLocalDocumentValueType::Array => Vec::new(),
    }
}

fn schema_actions(
    node: &DocumentSchemaNode,
    target_value_type: RuleLocalDocumentValueType,
) -> Vec<RuleDocumentActionCapability> {
    use RuleDocumentActionTargetKind::{Array, Node};
    use RuleLocalDocumentActionKind::{Append, Clear, Insert, Set};
    let mut actions = vec![
        RuleDocumentActionCapability {
            kind: Set,
            target_kind: Node,
            target_value_type,
            operand_value_type: Some(target_value_type),
        },
        RuleDocumentActionCapability {
            kind: Clear,
            target_kind: Node,
            target_value_type,
            operand_value_type: None,
        },
    ];
    if let DocumentSchemaNode::Array { items, .. } = node {
        let operand_value_type = schema_value_type(items);
        actions.extend([Insert, Append].map(|kind| RuleDocumentActionCapability {
            kind,
            target_kind: Array,
            target_value_type,
            operand_value_type: Some(operand_value_type),
        }));
    }
    actions
}

fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use intercept_proxy_domain::DocumentSchemaNode;

    use super::{
        RuleDocumentActionTargetKind, RuleLocalDocumentActionKind, RuleLocalDocumentValueType,
        document_schema_field_capabilities, local_document_type_capabilities,
    };

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

    #[test]
    fn schema_scalar_exposes_only_node_set_and_clear() {
        let schema = object_schema([("name", DocumentSchemaNode::String { title: None })]);

        let fields = document_schema_field_capabilities(&schema);
        let name = fields.iter().find(|field| field.path == "/name").unwrap();

        assert_eq!(
            name.actions
                .iter()
                .map(|action| action.kind)
                .collect::<Vec<_>>(),
            vec![
                RuleLocalDocumentActionKind::Set,
                RuleLocalDocumentActionKind::Clear,
            ]
        );
        assert!(name.actions.iter().all(|action| {
            action.target_kind == RuleDocumentActionTargetKind::Node
                && action.target_value_type == RuleLocalDocumentValueType::String
        }));
        assert_eq!(
            name.actions[0].operand_value_type,
            Some(RuleLocalDocumentValueType::String)
        );
        assert_eq!(name.actions[1].operand_value_type, None);
    }

    #[test]
    fn schema_array_actions_use_item_operand_type() {
        let schema = object_schema([(
            "tags",
            DocumentSchemaNode::Array {
                title: None,
                items: Box::new(DocumentSchemaNode::String { title: None }),
            },
        )]);

        let fields = document_schema_field_capabilities(&schema);
        let tags = fields.iter().find(|field| field.path == "/tags").unwrap();

        for kind in [
            RuleLocalDocumentActionKind::Insert,
            RuleLocalDocumentActionKind::Append,
        ] {
            let action = tags
                .actions
                .iter()
                .find(|action| action.kind == kind)
                .unwrap();
            assert_eq!(action.target_kind, RuleDocumentActionTargetKind::Array);
            assert_eq!(action.target_value_type, RuleLocalDocumentValueType::Array);
            assert_eq!(
                action.operand_value_type,
                Some(RuleLocalDocumentValueType::String)
            );
        }
    }

    #[test]
    fn nested_array_actions_keep_the_nested_item_type() {
        let schema = object_schema([(
            "matrix",
            DocumentSchemaNode::Array {
                title: None,
                items: Box::new(DocumentSchemaNode::Array {
                    title: None,
                    items: Box::new(DocumentSchemaNode::Number { title: None }),
                }),
            },
        )]);

        let fields = document_schema_field_capabilities(&schema);
        let matrix = fields.iter().find(|field| field.path == "/matrix").unwrap();
        let append = matrix
            .actions
            .iter()
            .find(|action| action.kind == RuleLocalDocumentActionKind::Append)
            .unwrap();
        assert_eq!(
            append.operand_value_type,
            Some(RuleLocalDocumentValueType::Array)
        );
        assert!(
            fields
                .iter()
                .find(|field| field.path == "/matrix/*")
                .unwrap()
                .actions
                .is_empty()
        );
    }

    #[test]
    fn schema_capabilities_keep_root_and_empty_name_property_distinct() {
        let schema = object_schema([("", DocumentSchemaNode::String { title: None })]);

        let fields = document_schema_field_capabilities(&schema);

        assert_eq!(fields[0].path, "");
        assert_eq!(fields[0].label, "/");
        assert_eq!(fields[1].path, "/");
        assert_eq!(fields[1].label, "/");
    }

    fn object_schema<const N: usize>(
        entries: [(&str, DocumentSchemaNode); N],
    ) -> DocumentSchemaNode {
        DocumentSchemaNode::Object {
            title: None,
            properties: entries
                .into_iter()
                .map(|(name, schema)| (name.to_owned(), schema))
                .collect::<BTreeMap<_, _>>(),
        }
    }
}
