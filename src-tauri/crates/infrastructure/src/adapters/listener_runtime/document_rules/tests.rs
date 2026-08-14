use std::{sync::Arc, thread};

use intercept_proxy_domain::{
    Document, DocumentAction, DocumentCondition, DocumentField, DocumentFieldName,
    DocumentFieldType, DocumentSchema, DocumentSchemaId, DocumentValue, ListenerId,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion, SocketDirection,
    SocketDocumentRuleDefinition, SocketDocumentRuleId, SocketDocumentRuleProgram,
};
use intercept_proxy_runtime::SocketConnectionIdentity;
use uuid::Uuid;

use super::*;

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("rules-test").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn schema() -> DocumentSchema {
    DocumentSchema::new(
        DocumentSchemaId::new("rules-test").unwrap(),
        1,
        "Rules test",
        vec![
            DocumentField::new(
                DocumentFieldName::new("request").unwrap(),
                DocumentFieldType::String,
                "Request",
            )
            .unwrap(),
            DocumentField::new(
                DocumentFieldName::new("response").unwrap(),
                DocumentFieldType::String,
                "Response",
            )
            .unwrap(),
        ],
    )
    .unwrap()
}

fn rule(
    listener_id: ListenerId,
    created_order: u64,
    conditions: Vec<DocumentCondition>,
    actions: Vec<DocumentAction>,
) -> SocketDocumentRuleDefinition {
    SocketDocumentRuleDefinition::new(
        SocketDocumentRuleId::new(),
        true,
        10,
        created_order,
        listener_id,
        package(),
        1,
        SocketDirection::Downstream,
        conditions,
        actions,
    )
    .unwrap()
}

fn connection(id: Uuid) -> SocketConnectionIdentity {
    SocketConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1),
        connection_id: id,
        peer_addr: "127.0.0.1:10000".parse().unwrap(),
    }
}

fn program(listener_id: ListenerId) -> Arc<SocketDocumentRuleProgram> {
    Arc::new(
        SocketDocumentRuleProgram::new(
            listener_id,
            package(),
            schema(),
            SocketDirection::Downstream,
            vec![
                rule(
                    listener_id,
                    1,
                    vec![DocumentCondition::Equals {
                        field: DocumentFieldName::new("request").unwrap(),
                        value: DocumentValue::String("sale".into()),
                    }],
                    vec![DocumentAction::RecordMatch],
                ),
                rule(
                    listener_id,
                    2,
                    Vec::new(),
                    vec![DocumentAction::SetField {
                        field: DocumentFieldName::new("response").unwrap(),
                        value: DocumentValue::String("approved".into()),
                    }],
                ),
            ],
        )
        .unwrap(),
    )
}

#[test]
fn local_empty_document_allows_static_assignment_but_field_condition_is_non_match() {
    let listener_id = ListenerId::new();
    let runtime =
        SocketDocumentRuleConnection::new(connection(Uuid::from_u128(10)), program(listener_id));

    let result = runtime.execute(runtime.empty_document()).unwrap();

    assert_eq!(result.matched_rule_ids().len(), 1);
    assert_eq!(
        result.document().get("response").unwrap(),
        &DocumentValue::String("approved".into())
    );
    assert!(!result.document().has("request").unwrap());
}

#[test]
fn runtime_rejects_cross_connection_and_every_frozen_binding_mismatch() {
    let listener_id = ListenerId::new();
    let first =
        SocketDocumentRuleConnection::new(connection(Uuid::from_u128(11)), program(listener_id));
    let second = SocketDocumentRuleConnection::new(
        connection(Uuid::from_u128(12)),
        Arc::clone(&first.program),
    );

    assert_eq!(
        second.execute(first.empty_document()).unwrap_err().code,
        ErrorCode::RuleInvalid
    );

    let mut wrong_listener = first.empty_document();
    wrong_listener.listener_id = ListenerId::new();
    assert!(first.execute(wrong_listener).is_err());
    let mut wrong_package = first.empty_document();
    wrong_package.package = ProtocolPackageRef {
        id: ProtocolPackageId::new("other-test").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    };
    assert!(first.execute(wrong_package).is_err());
    let mut wrong_direction = first.empty_document();
    wrong_direction.direction = SocketDirection::Upstream;
    assert!(first.execute(wrong_direction).is_err());

    let other_schema = DocumentSchema::new(
        DocumentSchemaId::new("other-test").unwrap(),
        1,
        "Other",
        vec![
            DocumentField::new(
                DocumentFieldName::new("other").unwrap(),
                DocumentFieldType::String,
                "Other",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let wrong_schema = first.bind_document(Document::new(other_schema));
    assert!(first.execute(wrong_schema).is_err());
}

#[test]
fn successive_frames_and_concurrent_connections_share_no_document_state() {
    let listener_id = ListenerId::new();
    let shared = program(listener_id);
    let first =
        SocketDocumentRuleConnection::new(connection(Uuid::from_u128(20)), Arc::clone(&shared));
    let mut decoded = Document::new(schema());
    decoded
        .set("request", DocumentValue::String("sale".into()))
        .unwrap();
    let first_result = first.execute(first.bind_document(decoded)).unwrap();
    assert_eq!(first_result.matched_rule_ids().len(), 2);

    let next = first.execute(first.empty_document()).unwrap();
    assert_eq!(next.matched_rule_ids().len(), 1);
    assert!(!next.document().has("request").unwrap());

    let handles = (30_u128..34)
        .map(|id| {
            let program = Arc::clone(&shared);
            thread::spawn(move || {
                let runtime =
                    SocketDocumentRuleConnection::new(connection(Uuid::from_u128(id)), program);
                runtime.execute(runtime.empty_document()).unwrap()
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        let result = handle.join().unwrap();
        assert_eq!(result.matched_rule_ids().len(), 1);
        assert!(!result.document().has("request").unwrap());
    }
}

#[test]
fn runtime_debug_exposes_binding_shape_but_never_document_values() {
    let listener_id = ListenerId::new();
    let runtime =
        SocketDocumentRuleConnection::new(connection(Uuid::from_u128(40)), program(listener_id));
    let mut document = Document::new(schema());
    document
        .set("request", DocumentValue::String("secret-sale".into()))
        .unwrap();
    let bound = runtime.bind_document(document);

    let runtime_debug = format!("{runtime:?}");
    let document_debug = format!("{bound:?}");
    assert!(runtime_debug.contains("rules-test"));
    assert!(document_debug.contains("rules-test"));
    assert!(!runtime_debug.contains("approved"));
    assert!(!runtime_debug.contains("sale"));
    assert!(!document_debug.contains("secret-sale"));
}

#[test]
fn factory_accepts_only_matching_directional_programs_and_creates_connections() {
    let listener_id = ListenerId::new();
    let upstream = Arc::new(
        SocketDocumentRuleProgram::new(
            listener_id,
            package(),
            schema(),
            SocketDirection::Upstream,
            Vec::new(),
        )
        .unwrap(),
    );
    let downstream = program(listener_id);
    let factory =
        SocketDocumentRuleConnectionFactory::new(Arc::clone(&upstream), Arc::clone(&downstream))
            .unwrap();
    let runtime = factory.connection(connection(Uuid::from_u128(50)), SocketDirection::Downstream);

    assert_eq!(
        runtime
            .execute(runtime.empty_document())
            .unwrap()
            .matched_rule_ids()
            .len(),
        1
    );
    assert!(
        SocketDocumentRuleConnectionFactory::new(downstream, upstream).is_err(),
        "swapped directions must be rejected"
    );
    assert!(
        SocketDocumentRuleConnectionFactory::new(
            Arc::new(
                SocketDocumentRuleProgram::new(
                    listener_id,
                    package(),
                    schema(),
                    SocketDirection::Upstream,
                    Vec::new(),
                )
                .unwrap(),
            ),
            program(ListenerId::new()),
        )
        .is_err(),
        "programs from different listeners must be rejected"
    );
}
