use std::{collections::BTreeMap, sync::Arc, thread};

use intercept_proxy_domain::{
    Document, DocumentAction, DocumentCondition, DocumentSchemaNode, DocumentValue, JsonPointer,
    ListenerId, ProtocolDirection, ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId,
    ProtocolDocumentRuleProgram, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    ProtocolRuleStage,
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

fn schema() -> DocumentSchemaNode {
    DocumentSchemaNode::Object {
        title: Some("Rules test".into()),
        properties: BTreeMap::from([
            (
                "request".into(),
                DocumentSchemaNode::String {
                    title: Some("Request".into()),
                },
            ),
            (
                "response".into(),
                DocumentSchemaNode::String {
                    title: Some("Response".into()),
                },
            ),
        ]),
    }
}

fn rule(
    listener_id: ListenerId,
    created_order: u64,
    conditions: Vec<DocumentCondition>,
    actions: Vec<DocumentAction>,
) -> ProtocolDocumentRuleDefinition {
    ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::new(),
        true,
        10,
        created_order,
        listener_id,
        package(),
        ProtocolDirection::Downstream,
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

fn program(listener_id: ListenerId) -> Arc<ProtocolDocumentRuleProgram> {
    Arc::new(
        ProtocolDocumentRuleProgram::new(
            listener_id,
            package(),
            schema(),
            ProtocolDirection::Downstream,
            vec![
                rule(
                    listener_id,
                    1,
                    vec![DocumentCondition::Equals {
                        field: JsonPointer::property("request"),
                        value: DocumentValue::String("sale".into()),
                    }],
                    vec![DocumentAction::RecordMatch],
                ),
                rule(
                    listener_id,
                    2,
                    Vec::new(),
                    vec![DocumentAction::SetField {
                        field: JsonPointer::property("response"),
                        value: DocumentValue::String("approved".into()),
                    }],
                ),
            ],
        )
        .unwrap(),
    )
}

fn empty_program(
    listener_id: ListenerId,
    stage: ProtocolRuleStage,
) -> Arc<ProtocolDocumentRuleProgram> {
    Arc::new(
        ProtocolDocumentRuleProgram::new_for_stage(
            listener_id,
            package(),
            schema(),
            stage,
            Vec::new(),
        )
        .unwrap(),
    )
}

fn factory(listener_id: ListenerId) -> ProtocolDocumentRuleConnectionFactory {
    ProtocolDocumentRuleConnectionFactory::new(
        empty_program(listener_id, ProtocolRuleStage::AppToProxy),
        empty_program(listener_id, ProtocolRuleStage::ProxyToUpstream),
        empty_program(listener_id, ProtocolRuleStage::UpstreamToProxy),
        program(listener_id),
    )
    .unwrap()
}

#[test]
fn local_empty_document_allows_static_assignment_but_field_condition_is_non_match() {
    let listener_id = ListenerId::new();
    let runtime = factory(listener_id).connection(
        connection(Uuid::from_u128(10)),
        ProtocolRuleStage::ProxyToApp,
    );

    let result = runtime.execute(runtime.empty_document()).unwrap();

    assert_eq!(result.matched_rule_ids().len(), 1);
    assert_eq!(
        result
            .document()
            .resolve(&JsonPointer::property("response"))
            .unwrap(),
        &DocumentValue::String("approved".into())
    );
    assert!(
        result
            .document()
            .resolve(&JsonPointer::property("request"))
            .is_err()
    );
}

#[test]
fn runtime_rejects_identity_mismatches_but_not_fields_missing_from_schema_metadata() {
    let listener_id = ListenerId::new();
    let factory = factory(listener_id);
    let first = factory.connection(
        connection(Uuid::from_u128(11)),
        ProtocolRuleStage::ProxyToApp,
    );
    let second = factory.connection(
        connection(Uuid::from_u128(12)),
        ProtocolRuleStage::ProxyToApp,
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
    let mut wrong_stage = first.empty_document();
    wrong_stage.stage = ProtocolRuleStage::AppToProxy;
    assert!(first.execute(wrong_stage).is_err());

    let incomplete_metadata_document = first.bind_document(Document::new(DocumentValue::Object(
        BTreeMap::from([("other".into(), DocumentValue::String("Other".into()))]),
    )));
    let result = first
        .execute(incomplete_metadata_document)
        .expect("schema metadata does not constrain the complete Document shape");
    assert_eq!(
        result
            .document()
            .resolve(&JsonPointer::property("other"))
            .unwrap(),
        &DocumentValue::String("Other".into())
    );
}

#[test]
fn successive_frames_and_concurrent_connections_share_no_document_state() {
    let listener_id = ListenerId::new();
    let shared = factory(listener_id);
    let first = shared.connection(
        connection(Uuid::from_u128(20)),
        ProtocolRuleStage::ProxyToApp,
    );
    let mut decoded = Document::new(DocumentValue::Object(BTreeMap::new()));
    decoded
        .set(
            &JsonPointer::property("request"),
            DocumentValue::String("sale".into()),
        )
        .unwrap();
    let first_result = first.execute(first.bind_document(decoded)).unwrap();
    assert_eq!(first_result.matched_rule_ids().len(), 2);

    let next = first.execute(first.empty_document()).unwrap();
    assert_eq!(next.matched_rule_ids().len(), 1);
    assert!(
        next.document()
            .resolve(&JsonPointer::property("request"))
            .is_err()
    );

    let handles = (30_u128..34)
        .map(|id| {
            let factory = shared.clone();
            thread::spawn(move || {
                let runtime = factory.connection(
                    connection(Uuid::from_u128(id)),
                    ProtocolRuleStage::ProxyToApp,
                );
                runtime.execute(runtime.empty_document()).unwrap()
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        let result = handle.join().unwrap();
        assert_eq!(result.matched_rule_ids().len(), 1);
        assert!(
            result
                .document()
                .resolve(&JsonPointer::property("request"))
                .is_err()
        );
    }
}

#[test]
fn runtime_debug_exposes_binding_shape_but_never_document_values() {
    let listener_id = ListenerId::new();
    let runtime = factory(listener_id).connection(
        connection(Uuid::from_u128(40)),
        ProtocolRuleStage::ProxyToApp,
    );
    let mut document = Document::new(DocumentValue::Object(BTreeMap::new()));
    document
        .set(
            &JsonPointer::property("request"),
            DocumentValue::String("secret-sale".into()),
        )
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
    let app_to_proxy = empty_program(listener_id, ProtocolRuleStage::AppToProxy);
    let proxy_to_upstream = empty_program(listener_id, ProtocolRuleStage::ProxyToUpstream);
    let upstream_to_proxy = empty_program(listener_id, ProtocolRuleStage::UpstreamToProxy);
    let proxy_to_app = program(listener_id);
    let factory = ProtocolDocumentRuleConnectionFactory::new(
        Arc::clone(&app_to_proxy),
        Arc::clone(&proxy_to_upstream),
        Arc::clone(&upstream_to_proxy),
        Arc::clone(&proxy_to_app),
    )
    .unwrap();
    let runtime = factory.connection(
        connection(Uuid::from_u128(50)),
        ProtocolRuleStage::ProxyToApp,
    );

    assert_eq!(
        runtime
            .execute(runtime.empty_document())
            .unwrap()
            .matched_rule_ids()
            .len(),
        1
    );
    assert!(
        ProtocolDocumentRuleConnectionFactory::new(
            proxy_to_upstream,
            app_to_proxy,
            upstream_to_proxy,
            proxy_to_app,
        )
        .is_err(),
        "swapped stages must be rejected"
    );
    assert!(
        ProtocolDocumentRuleConnectionFactory::new(
            empty_program(listener_id, ProtocolRuleStage::AppToProxy),
            empty_program(listener_id, ProtocolRuleStage::ProxyToUpstream),
            empty_program(listener_id, ProtocolRuleStage::UpstreamToProxy),
            empty_program(ListenerId::new(), ProtocolRuleStage::ProxyToApp),
        )
        .is_err(),
        "programs from different listeners must be rejected"
    );
}

#[test]
fn factory_debug_reports_all_four_stage_counts() {
    let listener_id = ListenerId::new();
    let debug = format!("{:?}", factory(listener_id));

    assert!(debug.contains("app_to_proxy: 0"));
    assert!(debug.contains("proxy_to_upstream: 0"));
    assert!(debug.contains("upstream_to_proxy: 0"));
    assert!(debug.contains("proxy_to_app: 2"));
}

#[test]
fn factory_rejects_different_schemas_within_one_direction() {
    let listener_id = ListenerId::new();
    let different_schema = DocumentSchemaNode::Object {
        title: Some("Different rules test".into()),
        properties: BTreeMap::from([
            (
                "request".into(),
                DocumentSchemaNode::String {
                    title: Some("Request".into()),
                },
            ),
            (
                "response".into(),
                DocumentSchemaNode::String {
                    title: Some("Response".into()),
                },
            ),
        ]),
    };
    let mismatched = Arc::new(
        ProtocolDocumentRuleProgram::new_for_stage(
            listener_id,
            package(),
            different_schema,
            ProtocolRuleStage::ProxyToUpstream,
            Vec::new(),
        )
        .unwrap(),
    );

    let error = ProtocolDocumentRuleConnectionFactory::new(
        empty_program(listener_id, ProtocolRuleStage::AppToProxy),
        mismatched,
        empty_program(listener_id, ProtocolRuleStage::UpstreamToProxy),
        empty_program(listener_id, ProtocolRuleStage::ProxyToApp),
    )
    .unwrap_err();

    assert_eq!(error.code, ErrorCode::RuleInvalid);
    assert!(error.field_errors.contains_key("factory.schema"));
}

#[test]
fn existing_connection_reads_replaced_rules_on_the_next_document() {
    let listener_id = ListenerId::new();
    let factory = factory(listener_id);
    let runtime = factory.connection(
        connection(Uuid::from_u128(60)),
        ProtocolRuleStage::ProxyToApp,
    );
    assert_eq!(
        runtime
            .execute(runtime.empty_document())
            .unwrap()
            .document()
            .resolve(&JsonPointer::property("response"))
            .unwrap(),
        &DocumentValue::String("approved".into())
    );

    let proxy_to_app = Arc::new(
        ProtocolDocumentRuleProgram::new(
            listener_id,
            package(),
            schema(),
            ProtocolDirection::Downstream,
            vec![rule(
                listener_id,
                1,
                Vec::new(),
                vec![DocumentAction::SetField {
                    field: JsonPointer::property("response"),
                    value: DocumentValue::String("declined".into()),
                }],
            )],
        )
        .unwrap(),
    );
    factory.replace(
        &ProtocolDocumentRuleConnectionFactory::new(
            empty_program(listener_id, ProtocolRuleStage::AppToProxy),
            empty_program(listener_id, ProtocolRuleStage::ProxyToUpstream),
            empty_program(listener_id, ProtocolRuleStage::UpstreamToProxy),
            proxy_to_app,
        )
        .unwrap(),
    );

    assert_eq!(
        runtime
            .execute(runtime.empty_document())
            .unwrap()
            .document()
            .resolve(&JsonPointer::property("response"))
            .unwrap(),
        &DocumentValue::String("declined".into())
    );
}
