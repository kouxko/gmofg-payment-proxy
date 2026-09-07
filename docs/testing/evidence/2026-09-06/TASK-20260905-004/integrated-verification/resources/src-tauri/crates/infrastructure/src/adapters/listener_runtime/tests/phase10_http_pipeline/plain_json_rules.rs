use super::production_shape::{
    Phase10ActorRules, actor_context, actor_pipeline, http_request_metadata,
};
use super::unified_working_exchange::http_rule;
use super::*;
use intercept_proxy_domain::{MatchField, MatchOperator};
// Keep the complete multi-generation/direction scenario together so its state transitions are reviewable.
#[allow(clippy::too_many_lines)]
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
    let mut request_message = Message::from_raw_http1_head(
        request_context.header.as_bytes(),
        Bytes::copy_from_slice(&request_context.wire_body),
    )
    .unwrap();
    upstream
        .prepare
        .prepare(
            &connection,
            &request,
            &mut request_message,
            &request_context,
            &request_document,
            &pipeline,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&request_message.body).unwrap(),
        json!({"customer":{"age":42.0,"result":"request"}})
    );

    let response_context = nested_json_context("HTTP/1.1 200 OK");
    let mut downstream = snapshot.create_downstream(identity).unwrap();
    let response_document = downstream.decode.decode(&response_context).await.unwrap();
    let mut response_message = Message::from_raw_http1_head(
        response_context.header.as_bytes(),
        Bytes::copy_from_slice(&response_context.wire_body),
    )
    .unwrap();
    downstream
        .prepare
        .prepare(
            &connection,
            &request,
            &mut response_message,
            &response_context,
            &response_document,
            &pipeline,
        )
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
