use super::production_shape::{
    Phase10ActorRules, actor_context, actor_pipeline, assert_production_changed_commit,
    execute_http_attempt, http_request_metadata, phase10_http_context,
    prepared_external_snapshot_for, prepared_external_snapshot_for_registration,
};
use super::*;
use intercept_proxy_domain::{MatchField, MatchOperator};

#[tokio::test]
async fn prior_header_action_is_visible_to_the_next_rule_condition() {
    use intercept_proxy_domain::{HttpAction, MatchField, MatchOperator};

    let listener = phase10_listener();
    let first = http_rule(
        &listener,
        "set first header",
        10,
        1,
        Condition::Http {
            field: MatchField::Method,
            operator: MatchOperator::Equals("POST".into()),
        },
        UnifiedAction::Http(HttpAction::SetHeader {
            name: "x-working".into(),
            value: "visible".into(),
        }),
    );
    let second = http_rule(
        &listener,
        "observe first header",
        20,
        2,
        Condition::Http {
            field: MatchField::Header("/x-working".into()),
            operator: MatchOperator::Equals("visible".into()),
        },
        UnifiedAction::Http(HttpAction::SetHeader {
            name: "x-observed".into(),
            value: "yes".into(),
        }),
    );
    let workspace = workspace_with_http_rules(&listener, vec![first, second]);
    let snapshot = prepared_external_snapshot_for(
        Arc::new(RecordingHttpRpc::new(false)),
        &workspace,
        &listener,
    )
    .await;
    let repository = Arc::new(Phase10ActorRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
            Some(Uuid::from_u128(1_020)),
            7,
            workspace.rule_definitions.clone(),
            workspace.http_runtime_rule_execution_order(),
        )),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = actor_pipeline(&snapshot, repository);
    pipeline.runtime_started(Uuid::from_u128(994)).await;

    let message = execute_http_attempt(&snapshot, &pipeline, 30).await;

    assert!(has_header(&message, b"x-working"));
    assert!(has_header(&message, b"x-observed"));
}

#[tokio::test]
async fn document_encode_is_the_only_final_body_write_after_ordered_actions() {
    use intercept_proxy_domain::HttpAction;

    let listener = phase10_listener();
    let document_rule = http_rule(
        &listener,
        "ordered document and HTTP body actions",
        10,
        1,
        Condition::Document {
            path: JsonPointer::property("value"),
            predicate: DocumentPredicate::String(StringPredicate {
                operator: StringOperator::Equal,
                value: "old".into(),
            }),
        },
        UnifiedAction::Document(DocumentMutation::Set {
            path: JsonPointer::property("value").into(),
            value: DocumentValue::String("new".into()),
        }),
    );
    let http_body_rule = http_rule(
        &listener,
        "ordered HTTP body action",
        20,
        2,
        Condition::Http {
            field: MatchField::Method,
            operator: MatchOperator::Equals("POST".into()),
        },
        UnifiedAction::Http(HttpAction::ReplaceBodyText(
            "must-not-overwrite-encode".into(),
        )),
    );
    let workspace = workspace_with_http_rules(&listener, vec![document_rule, http_body_rule]);

    assert_production_changed_commit(
        &listener,
        &workspace,
        workspace.rule_definitions.clone(),
        &HttpConnectionIdentity {
            runtime_epoch: Uuid::from_u128(1_021),
            connection_id: Uuid::from_u128(1_022),
            peer: "127.0.0.1:1902".into(),
        },
        br#"{"value":"new"}"#,
    )
    .await;
}

#[tokio::test]
async fn schema_free_http_hot_replace_keeps_root_rule_program() {
    let listener = phase10_listener();
    let registration = http_registration_without_schema();
    let initial = ProxyWorkspace::default();
    let snapshot = prepared_external_snapshot_for_registration(
        Arc::new(RecordingHttpRpc::new(false)),
        &initial,
        &listener,
        registration,
    )
    .await;
    let replacement = http_rule(
        &listener,
        "schema-free root hot replace",
        10,
        1,
        Condition::Http {
            field: MatchField::Method,
            operator: MatchOperator::Equals("POST".into()),
        },
        UnifiedAction::Document(DocumentMutation::Set {
            path: JsonPointer::root().into(),
            value: DocumentValue::String("root".into()),
        }),
    );
    let workspace = workspace_with_http_rules(&listener, vec![replacement]);
    let adapter = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));

    let replacement = snapshot
        .compile_replacement(&adapter, &workspace, &listener)
        .await
        .expect("schema-free hot replace");
    snapshot.publish_replacement(replacement);

    assert_eq!(snapshot.rule_count(ProtocolDirection::Upstream), 1);
}

#[tokio::test]
async fn existing_http_connection_observes_rule_created_after_capabilities() {
    let listener = phase10_listener();
    let initial = ProxyWorkspace::default();
    let snapshot =
        prepared_external_snapshot_for(Arc::new(RecordingHttpRpc::new(false)), &initial, &listener)
            .await;
    let identity = HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1_031),
        connection_id: Uuid::from_u128(1_032),
        peer: "127.0.0.1:1902".into(),
    };
    let mut capabilities = snapshot.create_upstream(identity.clone()).unwrap();
    let replacement = http_rule(
        &listener,
        "keep-alive create",
        10,
        1,
        Condition::Http {
            field: MatchField::Method,
            operator: MatchOperator::Equals("POST".into()),
        },
        UnifiedAction::Document(DocumentMutation::Set {
            path: JsonPointer::property("value").into(),
            value: DocumentValue::String("created".into()),
        }),
    );
    let workspace = workspace_with_http_rules(&listener, vec![replacement]);
    let adapter = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    let replacement = snapshot
        .compile_replacement(&adapter, &workspace, &listener)
        .await
        .unwrap();
    snapshot.publish_replacement(replacement);
    let repository = Arc::new(Phase10ActorRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
            Some(workspace.id.as_uuid()),
            workspace.revision.get(),
            workspace.rule_definitions.clone(),
            workspace.http_runtime_rule_execution_order(),
        )),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = actor_pipeline(&snapshot, repository);
    pipeline.runtime_started(identity.runtime_epoch).await;
    let connection = actor_context(&snapshot, &identity);
    pipeline.connection_opened(&connection).await;
    let context = phase10_http_context();
    let document = capabilities.decode.decode(&context).await.unwrap();
    capabilities.rules.apply(document).await.unwrap();
    let mut message = Message::from_raw_http1_head(
        context.header.as_bytes(),
        Bytes::copy_from_slice(&context.wire_body),
    )
    .unwrap();

    pipeline
        .apply_request_policy(&connection, &http_request_metadata(), &mut message)
        .await
        .unwrap();

    assert_eq!(message.body.as_ref(), br#"{"value":"created"}"#);
}

fn http_rule(
    listener: &ProxyListener,
    name: &str,
    priority: i32,
    created_order: u64,
    condition: Condition,
    action: UnifiedAction,
) -> RuleDefinition {
    RuleDefinition::create(
        RuleDefinitionDraft {
            name: name.into(),
            enabled: true,
            priority,
            listener_id: listener.id,
            stage: RuleStage::ProxyToUpstream,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition,
                action,
            }),
        },
        created_order,
    )
    .unwrap()
}

fn has_header(message: &Message, name: &[u8]) -> bool {
    message
        .headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(name))
}

#[tokio::test]
async fn plain_http_json_body_matches_manual_pointer_on_request_and_response() {
    let listener = ProxyListener::default();
    let condition = || Condition::Document {
        path: JsonPointer::parse("/customer/age").unwrap(),
        predicate: DocumentPredicate::Number(intercept_proxy_domain::NumberPredicate {
            operator: intercept_proxy_domain::NumberOperator::Equal,
            value: intercept_proxy_domain::DocumentNumber::new(42.0).unwrap(),
        }),
    };
    let rule = |stage, order, value: &str| {
        RuleDefinition::create(
            RuleDefinitionDraft {
                name: format!("plain {stage:?}"),
                enabled: true,
                priority: 1,
                listener_id: listener.id,
                stage,
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    condition: condition(),
                    action: UnifiedAction::Document(DocumentMutation::Set {
                        path: JsonPointer::parse("/customer/result").unwrap().into(),
                        value: DocumentValue::String(value.into()),
                    }),
                }),
            },
            order,
        )
        .unwrap()
    };
    let workspace = workspace_with_http_rules(
        &listener,
        vec![
            rule(RuleStage::ProxyToUpstream, 1, "request"),
            rule(RuleStage::ProxyToApp, 2, "response"),
        ],
    );
    let snapshot = prepared_plain_snapshot(&workspace, &listener).await;
    let repository = Arc::new(Phase10ActorRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
            Some(workspace.id.as_uuid()),
            workspace.revision.get(),
            workspace.rule_definitions.clone(),
            workspace.runtime_rule_execution_order(),
        )),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = actor_pipeline(&snapshot, repository);
    let identity = HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1_041),
        connection_id: Uuid::from_u128(1_042),
        peer: "127.0.0.1:1902".into(),
    };
    pipeline.runtime_started(identity.runtime_epoch).await;
    let connection = actor_context(&snapshot, &identity);
    pipeline.connection_opened(&connection).await;
    let request = http_request_metadata();

    let request_context = nested_json_context("POST /customer HTTP/1.1");
    let mut upstream = snapshot.create_upstream(identity.clone()).unwrap();
    let request_document = upstream.decode.decode(&request_context).await.unwrap();
    upstream.rules.apply(request_document).await.unwrap();
    let mut request_message = Message::from_raw_http1_head(
        request_context.header.as_bytes(),
        Bytes::copy_from_slice(&request_context.wire_body),
    )
    .unwrap();
    pipeline
        .apply_request_policy(&connection, &request, &mut request_message)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&request_message.body).unwrap(),
        json!({"customer":{"age":42.0,"result":"request"}})
    );

    let response_context = nested_json_context("HTTP/1.1 200 OK");
    let mut downstream = snapshot.create_downstream(identity).unwrap();
    let response_document = downstream.decode.decode(&response_context).await.unwrap();
    downstream.rules.apply(response_document).await.unwrap();
    let mut response_message = Message::from_raw_http1_head(
        response_context.header.as_bytes(),
        Bytes::copy_from_slice(&response_context.wire_body),
    )
    .unwrap();
    pipeline
        .apply_response_policy(&connection, &request, &mut response_message)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response_message.body).unwrap(),
        json!({"customer":{"age":42.0,"result":"response"}})
    );
}

#[tokio::test]
async fn plain_http_document_rule_rejects_invalid_json_without_fallback() {
    let listener = ProxyListener::default();
    let definition = http_rule(
        &listener,
        "plain invalid json",
        1,
        1,
        Condition::Http {
            field: MatchField::Method,
            operator: MatchOperator::Equals("POST".into()),
        },
        UnifiedAction::Document(DocumentMutation::Set {
            path: JsonPointer::root().into(),
            value: DocumentValue::Null(()),
        }),
    );
    let workspace = workspace_with_http_rules(&listener, vec![definition]);
    let snapshot = prepared_plain_snapshot(&workspace, &listener).await;
    let identity = HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1_043),
        connection_id: Uuid::from_u128(1_044),
        peer: "127.0.0.1:1902".into(),
    };
    let mut upstream = snapshot.create_upstream(identity).unwrap();
    let context = HttpContext {
        header: "POST / HTTP/1.1\r\nContent-Type: application/json\r\n\r\n".into(),
        body: "{".into(),
        body_is_utf8: true,
        wire_body: b"{".to_vec(),
    };

    let error = upstream.decode.decode(&context).await.unwrap_err();

    assert!(error.message.starts_with("JSON_INVALID\n"));
}

fn nested_json_context(start_line: &str) -> HttpContext {
    let body = br#"{"customer":{"age":42}}"#;
    HttpContext {
        header: format!("{start_line}\r\nContent-Type: application/json; charset=utf-8\r\n\r\n"),
        body: String::from_utf8(body.to_vec()).unwrap(),
        body_is_utf8: true,
        wire_body: body.to_vec(),
    }
}
