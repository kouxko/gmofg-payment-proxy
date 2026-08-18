use super::*;
use crate::{
    DocumentField, DocumentFieldType, DocumentSchemaId, ErrorCode, ProtocolPackageId,
    ProtocolPackageVersion,
};
use uuid::Uuid;

fn package(version: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("iso8583-standard").unwrap(),
        version: ProtocolPackageVersion::new(version).unwrap(),
    }
}

fn schema() -> DocumentSchema {
    DocumentSchema::new(
        DocumentSchemaId::new("payment-message").unwrap(),
        7,
        "Payment Message",
        [
            ("text", DocumentFieldType::String),
            ("amount", DocumentFieldType::Int),
            ("approved", DocumentFieldType::Bool),
            ("payload", DocumentFieldType::Blob),
        ]
        .into_iter()
        .map(|(name, kind)| {
            DocumentField::new(DocumentFieldName::new(name).unwrap(), kind, name).unwrap()
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

fn rule(
    id: u128,
    listener_id: ListenerId,
    direction: ProtocolDirection,
    conditions: Vec<DocumentCondition>,
    actions: Vec<DocumentAction>,
) -> Result<ProtocolDocumentRuleDefinition, DomainError> {
    ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(id)),
        true,
        10,
        u64::try_from(id).unwrap(),
        listener_id,
        package("1.2.3"),
        7,
        direction,
        conditions,
        actions,
    )
}

#[test]
fn four_document_types_accept_exact_equals_and_set_field() {
    let listener_id = ListenerId::new();
    let cases = [
        ("text", DocumentValue::String("ok".into())),
        ("amount", DocumentValue::Int(1000)),
        ("approved", DocumentValue::Bool(true)),
        ("payload", DocumentValue::Blob(vec![0, 1, 255])),
    ];
    for (field, value) in cases {
        let candidate = rule(
            1,
            listener_id,
            ProtocolDirection::Upstream,
            vec![condition(field, value.clone())],
            vec![set(field, value)],
        )
        .unwrap();
        candidate.validate_against_schema(&schema()).unwrap();
    }
}

#[test]
fn schema_validation_rejects_unknown_fields_wrong_types_and_versions() {
    let listener_id = ListenerId::new();
    for candidate in [
        rule(
            1,
            listener_id,
            ProtocolDirection::Upstream,
            vec![condition("unknown", DocumentValue::String("x".into()))],
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
        rule(
            2,
            listener_id,
            ProtocolDirection::Upstream,
            vec![condition("amount", DocumentValue::String("1000".into()))],
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
        rule(
            3,
            listener_id,
            ProtocolDirection::Upstream,
            Vec::new(),
            vec![set("approved", DocumentValue::Int(1))],
        )
        .unwrap(),
    ] {
        assert_eq!(
            candidate
                .validate_against_schema(&schema())
                .unwrap_err()
                .code,
            ErrorCode::RuleInvalid
        );
    }

    let mut json = serde_json::to_value(
        rule(
            4,
            listener_id,
            ProtocolDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
    )
    .unwrap();
    json["schema_version"] = serde_json::json!(8);
    let versioned: ProtocolDocumentRuleDefinition = serde_json::from_value(json).unwrap();
    assert!(versioned.validate_against_schema(&schema()).is_err());
}

#[test]
fn structure_rejects_duplicate_conditions_empty_actions_and_oversize_values() {
    let listener_id = ListenerId::new();
    assert!(
        rule(
            1,
            listener_id,
            ProtocolDirection::Upstream,
            vec![
                condition("amount", DocumentValue::Int(1)),
                condition("amount", DocumentValue::Int(2)),
            ],
            vec![DocumentAction::RecordMatch],
        )
        .is_err()
    );
    assert!(
        rule(
            2,
            listener_id,
            ProtocolDirection::Upstream,
            Vec::new(),
            Vec::new()
        )
        .is_err()
    );
    assert!(
        rule(
            3,
            listener_id,
            ProtocolDirection::Upstream,
            vec![condition(
                "text",
                DocumentValue::String("x".repeat(MAX_PROTOCOL_DOCUMENT_RULE_STRING_BYTES + 1)),
            )],
            vec![DocumentAction::RecordMatch],
        )
        .is_err()
    );
    assert!(
        rule(
            4,
            listener_id,
            ProtocolDirection::Upstream,
            Vec::new(),
            vec![set(
                "payload",
                DocumentValue::Blob(vec![0; MAX_PROTOCOL_DOCUMENT_RULE_BLOB_BYTES + 1]),
            )],
        )
        .is_err()
    );
    for value in [-9_007_199_254_740_992, 9_007_199_254_740_992] {
        assert!(
            rule(
                5,
                listener_id,
                ProtocolDirection::Upstream,
                vec![condition("amount", DocumentValue::Int(value))],
                vec![DocumentAction::RecordMatch],
            )
            .is_err()
        );
        assert!(
            rule(
                6,
                listener_id,
                ProtocolDirection::Upstream,
                Vec::new(),
                vec![set("amount", DocumentValue::Int(value))],
            )
            .is_err()
        );
    }
    for value in [-9_007_199_254_740_991, 9_007_199_254_740_991] {
        rule(
            7,
            listener_id,
            ProtocolDirection::Upstream,
            vec![condition("amount", DocumentValue::Int(value))],
            vec![set("amount", DocumentValue::Int(value))],
        )
        .unwrap();
    }
}

#[test]
fn actions_preserve_declared_order_and_support_empty_conditions() {
    let candidate = rule(
        1,
        ListenerId::new(),
        ProtocolDirection::Downstream,
        Vec::new(),
        vec![
            DocumentAction::ClearDocument,
            set("text", DocumentValue::String("00".into())),
            DocumentAction::RecordMatch,
        ],
    )
    .unwrap();
    candidate.validate_against_schema(&schema()).unwrap();
    assert!(candidate.conditions().is_empty());
    assert!(matches!(
        candidate.actions()[0],
        DocumentAction::ClearDocument
    ));
    assert!(matches!(
        candidate.actions()[1],
        DocumentAction::SetField { .. }
    ));
    assert!(matches!(
        candidate.actions()[2],
        DocumentAction::RecordMatch
    ));
}

#[test]
fn update_and_toggle_preserve_identity_and_enforce_revision() {
    let listener_id = ListenerId::new();
    let mut current = rule(
        1,
        listener_id,
        ProtocolDirection::Upstream,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    let identity = current.rule_id();
    let created_order = current.created_order();
    assert_eq!(
        current.set_enabled(Revision::INITIAL, false).unwrap(),
        Revision::new(2)
    );
    assert!(!current.enabled());
    assert_eq!(
        current
            .set_enabled(Revision::INITIAL, true)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );

    let replacement = rule(
        99,
        listener_id,
        ProtocolDirection::Upstream,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    assert_eq!(
        current
            .update(Revision::new(2), replacement.to_draft())
            .unwrap(),
        Revision::new(3)
    );
    assert_eq!(current.rule_id(), identity);
    assert_eq!(current.created_order(), created_order);
    assert_eq!(current.direction(), ProtocolDirection::Upstream);

    let mut switched = current.to_draft();
    switched.stage = ProtocolRuleStage::ProxyToApp;
    assert_eq!(
        current.update(Revision::new(3), switched).unwrap_err().code,
        ErrorCode::RuleInvalid
    );
    assert_eq!(current.revision(), Revision::new(3));
}

#[test]
fn revision_exhaustion_rejects_update_and_toggle_without_side_effects() {
    let current = rule(
        1,
        ListenerId::new(),
        ProtocolDirection::Upstream,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();

    for revision in [
        Revision::new(9_007_199_254_740_991),
        Revision::new(u64::MAX),
    ] {
        let mut update_target = ProtocolDocumentRuleDefinition {
            revision,
            ..current.clone()
        };
        let before = update_target.clone();
        assert_eq!(
            update_target
                .update(revision, before.to_draft())
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(update_target, before);

        let mut toggle_target = ProtocolDocumentRuleDefinition {
            revision,
            ..current.clone()
        };
        let before = toggle_target.clone();
        assert_eq!(
            toggle_target.toggle(revision, false).unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(toggle_target, before);
    }
}

#[test]
fn workspace_remap_rebinds_only_listener_identity() {
    let original_listener = ListenerId::new();
    let replacement_listener = ListenerId::new();
    let mut current = rule(
        1,
        original_listener,
        ProtocolDirection::Downstream,
        vec![condition("amount", DocumentValue::Int(1))],
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    let before = current.clone();

    current
        .rebind_listener_for_workspace_remap(replacement_listener)
        .unwrap();

    assert_eq!(current.listener_id(), replacement_listener);
    assert_eq!(current.rule_id(), before.rule_id());
    assert_eq!(current.revision(), before.revision());
    assert_eq!(current.created_order(), before.created_order());
    assert_eq!(current.package(), before.package());
    assert_eq!(current.schema_version(), before.schema_version());
    assert_eq!(current.direction(), before.direction());
    assert_eq!(current.conditions(), before.conditions());
    assert_eq!(current.actions(), before.actions());
}

#[test]
fn deterministic_sort_uses_priority_created_order_and_rule_id() {
    let listener_id = ListenerId::new();
    let make = |id: u128, priority: i32, created_order: u64| {
        let mut value = serde_json::to_value(
            rule(
                id,
                listener_id,
                ProtocolDirection::Upstream,
                Vec::new(),
                vec![DocumentAction::RecordMatch],
            )
            .unwrap(),
        )
        .unwrap();
        value["priority"] = priority.into();
        value["created_order"] = created_order.into();
        serde_json::from_value::<ProtocolDocumentRuleDefinition>(value).unwrap()
    };
    let mut rules = vec![make(3, 1, 2), make(2, 0, 9), make(1, 1, 2)];
    sort_protocol_document_rules(&mut rules);
    assert_eq!(
        rules
            .iter()
            .map(ProtocolDocumentRuleDefinition::rule_id)
            .collect::<Vec<_>>(),
        vec![
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(2)),
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(1)),
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(3)),
        ]
    );
}

mod serde;
mod workspace;
