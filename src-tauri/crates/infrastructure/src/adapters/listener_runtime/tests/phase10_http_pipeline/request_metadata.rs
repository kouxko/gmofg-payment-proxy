use super::production_shape::{
    Phase10ActorRules, actor_context, actor_pipeline, http_request_metadata,
    prepared_external_snapshot_for,
};
use super::*;

#[tokio::test]
async fn production_response_rule_matches_flat_condition_against_associated_request_metadata() {
    use intercept_proxy_domain::{
        Condition, HttpAction, MatchField, MatchOperator, RuleStage, UnifiedAction,
    };

    let listener = phase10_listener();
    let definition = RuleDefinition::create(
        RuleDefinitionDraft {
            name: "response request-target".into(),
            enabled: true,
            priority: 1,
            listener_id: listener.id,
            stage: RuleStage::ProxyToApp,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition: Condition::Http {
                    field: MatchField::RequestTarget,
                    operator: MatchOperator::Wildcard("/phase*".into()),
                },
                action: UnifiedAction::Http(HttpAction::SetHeader {
                    name: "x-response-rule".into(),
                    value: "matched".into(),
                }),
            }),
        },
        1,
    )
    .expect("valid response rule");
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        rule_definitions: vec![definition],
        rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    let snapshot = prepared_external_snapshot_for(
        Arc::new(RecordingHttpRpc::new(false)),
        &workspace,
        &listener,
    )
    .await;
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
        runtime_epoch: Uuid::from_u128(971),
        connection_id: Uuid::from_u128(972),
        peer: "127.0.0.1:1902".into(),
    };
    pipeline.runtime_started(identity.runtime_epoch).await;
    let connection = actor_context(&snapshot, &identity);
    pipeline.connection_opened(&connection).await;
    let request = http_request_metadata();
    let mut request_message = Message::from_raw_http1_head(
        b"POST /phase10 HTTP/1.1\r\ncontent-length: 0\r\n\r\n",
        Bytes::new(),
    )
    .expect("request message");
    pipeline
        .apply_request_policy(&connection, &request, &mut request_message)
        .await
        .expect("request policy");
    let mut response = Message::from_raw_http1_head(
        b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n",
        Bytes::new(),
    )
    .expect("response message");

    pipeline
        .apply_response_policy(&connection, &request, &mut response)
        .await
        .expect("response policy");

    assert!(response.headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case(b"x-response-rule") && header.value.as_ref() == b"matched"
    }));
}
