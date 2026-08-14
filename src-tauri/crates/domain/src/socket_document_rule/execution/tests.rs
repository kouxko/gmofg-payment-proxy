use super::*;
use crate::{
    DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchemaId, DocumentValue,
    ProtocolPackageId, ProtocolPackageVersion,
};
use uuid::Uuid;

fn package(version: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("binary-demo").unwrap(),
        version: ProtocolPackageVersion::new(version).unwrap(),
    }
}

fn schema() -> DocumentSchema {
    DocumentSchema::new(
        DocumentSchemaId::new("message").unwrap(),
        7,
        "Message",
        [
            ("text", DocumentFieldType::String),
            ("amount", DocumentFieldType::Int),
            ("approved", DocumentFieldType::Bool),
            ("payload", DocumentFieldType::Blob),
        ]
        .into_iter()
        .map(|(name, field_type)| {
            DocumentField::new(DocumentFieldName::new(name).unwrap(), field_type, name).unwrap()
        })
        .collect(),
    )
    .unwrap()
}

fn condition(field: &str, value: DocumentValue) -> DocumentCondition {
    DocumentCondition::Equals {
        field: DocumentFieldName::new(field).unwrap(),
        value,
    }
}

fn set(field: &str, value: DocumentValue) -> DocumentAction {
    DocumentAction::SetField {
        field: DocumentFieldName::new(field).unwrap(),
        value,
    }
}

#[allow(clippy::too_many_arguments)]
fn rule(
    id: u128,
    listener_id: ListenerId,
    package: ProtocolPackageRef,
    schema_version: u32,
    direction: SocketDirection,
    enabled: bool,
    priority: i32,
    created_order: u64,
    conditions: Vec<DocumentCondition>,
    actions: Vec<DocumentAction>,
) -> SocketDocumentRuleDefinition {
    SocketDocumentRuleDefinition::new(
        SocketDocumentRuleId::from_uuid(Uuid::from_u128(id)),
        enabled,
        priority,
        created_order,
        listener_id,
        package,
        schema_version,
        direction,
        conditions,
        actions,
    )
    .unwrap()
}

fn program(
    listener_id: ListenerId,
    rules: Vec<SocketDocumentRuleDefinition>,
) -> SocketDocumentRuleProgram {
    SocketDocumentRuleProgram::new(
        listener_id,
        package("1.2.3"),
        schema(),
        SocketDirection::Downstream,
        rules,
    )
    .unwrap()
}

#[test]
fn equals_and_set_field_support_all_four_document_types() {
    let listener_id = ListenerId::new();
    let conditions = vec![
        condition("text", DocumentValue::String("request".into())),
        condition("amount", DocumentValue::Int(100)),
        condition("approved", DocumentValue::Bool(false)),
        condition("payload", DocumentValue::Blob(vec![0, 1])),
    ];
    let actions = vec![
        set("text", DocumentValue::String("response".into())),
        set("amount", DocumentValue::Int(200)),
        set("approved", DocumentValue::Bool(true)),
        set("payload", DocumentValue::Blob(vec![2, 3])),
    ];
    let candidate = rule(
        1,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        1,
        conditions,
        actions,
    );
    let program = program(listener_id, vec![candidate.clone()]);
    let mut input = Document::new(schema());
    input
        .set("text", DocumentValue::String("request".into()))
        .unwrap();
    input.set("amount", DocumentValue::Int(100)).unwrap();
    input.set("approved", DocumentValue::Bool(false)).unwrap();
    input
        .set("payload", DocumentValue::Blob(vec![0, 1]))
        .unwrap();

    let result = program.execute(input).unwrap();

    assert_eq!(result.matched_rule_ids(), &[candidate.rule_id()]);
    assert_eq!(
        result.document().get("text").unwrap(),
        &DocumentValue::String("response".into())
    );
    assert_eq!(
        result.document().get("amount").unwrap(),
        &DocumentValue::Int(200)
    );
    assert_eq!(
        result.document().get("approved").unwrap(),
        &DocumentValue::Bool(true)
    );
    assert_eq!(
        result.document().get("payload").unwrap(),
        &DocumentValue::Blob(vec![2, 3])
    );
}

#[test]
fn conditions_are_and_empty_is_always_and_unassigned_is_non_match() {
    let listener_id = ListenerId::new();
    let and_rule = rule(
        1,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        1,
        vec![
            condition("amount", DocumentValue::Int(10)),
            condition("approved", DocumentValue::Bool(true)),
        ],
        vec![DocumentAction::RecordMatch],
    );
    let always_rule = rule(
        2,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        1,
        2,
        Vec::new(),
        vec![set("text", DocumentValue::String("always".into()))],
    );
    let program = program(listener_id, vec![always_rule.clone(), and_rule]);
    let mut input = Document::new(schema());
    input.set("amount", DocumentValue::Int(10)).unwrap();

    let result = program.execute(input).unwrap();

    assert_eq!(result.matched_rule_ids(), &[always_rule.rule_id()]);
    assert_eq!(
        result.document().get("text").unwrap(),
        &DocumentValue::String("always".into())
    );
}

#[test]
fn declared_action_order_applies_and_clear_preserves_schema() {
    let listener_id = ListenerId::new();
    let candidate = rule(
        1,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        1,
        Vec::new(),
        vec![
            set("amount", DocumentValue::Int(99)),
            DocumentAction::ClearDocument,
            set("text", DocumentValue::String("after-clear".into())),
            DocumentAction::RecordMatch,
        ],
    );
    let program = program(listener_id, vec![candidate]);
    let input_schema = schema();
    let mut input = Document::new(input_schema.clone());
    input.set("approved", DocumentValue::Bool(true)).unwrap();

    let result = program.execute(input).unwrap();

    assert_eq!(result.document().schema(), &input_schema);
    assert!(!result.document().has("amount").unwrap());
    assert!(!result.document().has("approved").unwrap());
    assert_eq!(
        result.document().get("text").unwrap(),
        &DocumentValue::String("after-clear".into())
    );
}

#[test]
fn all_matches_execute_in_sorted_order_and_later_rules_can_overwrite() {
    let listener_id = ListenerId::new();
    let early = rule(
        1,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        1,
        Vec::new(),
        vec![set("amount", DocumentValue::Int(1))],
    );
    let middle = rule(
        2,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        2,
        vec![condition("amount", DocumentValue::Int(1))],
        vec![set("amount", DocumentValue::Int(2))],
    );
    let late = rule(
        3,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        10,
        3,
        Vec::new(),
        vec![set("amount", DocumentValue::Int(3))],
    );
    let disabled = rule(
        4,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        false,
        -10,
        4,
        Vec::new(),
        vec![set("amount", DocumentValue::Int(999))],
    );
    let program = program(
        listener_id,
        vec![late.clone(), disabled, middle.clone(), early.clone()],
    );

    let result = program.execute(Document::new(schema())).unwrap();

    assert_eq!(
        result.matched_rule_ids(),
        &[early.rule_id(), middle.rule_id(), late.rule_id()]
    );
    assert_eq!(
        result.document().get("amount").unwrap(),
        &DocumentValue::Int(3)
    );
    assert_eq!(program.rules()[0].priority(), -10);
}

mod isolation;
