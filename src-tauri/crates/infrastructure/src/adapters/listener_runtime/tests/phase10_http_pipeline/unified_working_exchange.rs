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
    let mut message = Message::from_raw_http1_head(
        context.header.as_bytes(),
        Bytes::copy_from_slice(&context.wire_body),
    )
    .unwrap();

    capabilities
        .prepare
        .prepare(
            &connection,
            &http_request_metadata(),
            &mut message,
            &context,
            &document,
            &pipeline,
        )
        .await
        .unwrap();

    assert_eq!(message.body.as_ref(), br#"{"value":"created"}"#);
}

#[tokio::test]
async fn existing_plain_http_connection_switches_document_capability_after_hot_replacement() {
    let listener = ProxyListener::default();
    let initial = workspace_with_http_rules(&listener, Vec::new());
    let snapshot = prepared_plain_snapshot(&initial, &listener).await;
    let repository = Arc::new(Phase10ActorRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
            Some(initial.id.as_uuid()),
            initial.revision.get(),
            Vec::new(),
            initial.http_runtime_rule_execution_order(),
        )),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = actor_pipeline(&snapshot, Arc::clone(&repository));
    let identity = HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1_033),
        connection_id: Uuid::from_u128(1_034),
        peer: "127.0.0.1:1902".into(),
    };
    pipeline.runtime_started(identity.runtime_epoch).await;
    let connection = actor_context(&snapshot, &identity);
    pipeline.connection_opened(&connection).await;
    let mut upstream = snapshot.create_upstream(identity).unwrap();

    let plain_before = HttpContext {
        header: "POST /phase10 HTTP/1.1\r\nContent-Type: text/plain\r\n\r\n".into(),
        body: "plain-before".into(),
        body_is_utf8: true,
        wire_body: b"plain-before".to_vec(),
    };
    let message =
        apply_existing_plain_request(&mut upstream, &pipeline, &connection, &plain_before)
            .await
            .unwrap();
    assert_eq!(message.body.as_ref(), b"plain-before");

    let replacement_rule = http_rule(
        &listener,
        "plain keep-alive add Document rule",
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
            value: DocumentValue::String("created".into()),
        }),
    );
    let with_document_rule = workspace_with_http_rules(&listener, vec![replacement_rule]);
    let adapter = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    let replacement = snapshot
        .compile_replacement(&adapter, &with_document_rule, &listener)
        .await
        .unwrap();
    snapshot.publish_replacement(replacement);
    *repository.snapshot.lock() = RuleRuntimeSnapshot::with_collection_identity_and_order(
        Some(with_document_rule.id.as_uuid()),
        with_document_rule.revision.get(),
        with_document_rule.rule_definitions.clone(),
        with_document_rule.http_runtime_rule_execution_order(),
    );

    let json_after_add = phase10_http_context();
    let message =
        apply_existing_plain_request(&mut upstream, &pipeline, &connection, &json_after_add)
            .await
            .expect("the existing Plain connection must acquire the new Document transaction");
    assert_eq!(message.body.as_ref(), br#"{"value":"created"}"#);

    let after_removal = workspace_with_http_rules(&listener, Vec::new());
    let replacement = snapshot
        .compile_replacement(&adapter, &after_removal, &listener)
        .await
        .unwrap();
    snapshot.publish_replacement(replacement);
    *repository.snapshot.lock() = RuleRuntimeSnapshot::with_collection_identity_and_order(
        Some(after_removal.id.as_uuid()),
        after_removal.revision.get(),
        Vec::new(),
        after_removal.http_runtime_rule_execution_order(),
    );

    let plain_after_removal = HttpContext {
        header: "POST /phase10 HTTP/1.1\r\nContent-Type: text/plain\r\n\r\n".into(),
        body: "plain-after-removal".into(),
        body_is_utf8: true,
        wire_body: b"plain-after-removal".to_vec(),
    };
    let message =
        apply_existing_plain_request(&mut upstream, &pipeline, &connection, &plain_after_removal)
            .await
            .unwrap();
    assert_eq!(message.body.as_ref(), b"plain-after-removal");
}

async fn apply_existing_plain_request(
    upstream: &mut intercept_proxy_runtime::HttpDirectionCapabilities<
        intercept_proxy_exchange::Upstream,
    >,
    pipeline: &RuntimePipelineAdapter,
    connection: &ConnectionContext,
    context: &HttpContext,
) -> Result<Message, intercept_proxy_exchange::Error> {
    let document = upstream.decode.decode(context).await.unwrap();
    let mut message = Message::from_raw_http1_head(
        context.header.as_bytes(),
        Bytes::copy_from_slice(&context.wire_body),
    )
    .unwrap();
    upstream
        .prepare
        .prepare(
            connection,
            &http_request_metadata(),
            &mut message,
            context,
            &document,
            pipeline,
        )
        .await?;
    Ok(message)
}

pub(super) fn http_rule(
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
