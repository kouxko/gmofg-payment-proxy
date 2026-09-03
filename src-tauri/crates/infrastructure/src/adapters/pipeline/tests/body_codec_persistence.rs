#[tokio::test(flavor = "current_thread")]
async fn frozen_body_codec_pipeline_progresses_while_sqlite_executor_is_occupied() {
    use std::sync::mpsc;

    use intercept_proxy_domain::{
        BodyCodecKind, HttpListenerSettings, ListenerDataPlane, ListenerId, ProxyListener,
    };
    use tokio::sync::oneshot;

    use crate::{InfrastructureError, SqliteExecutor, SqliteStore, adapters::WorkspaceBodyCodecResolver};

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let executor = SqliteExecutor::new(store);
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let occupied = tokio::spawn(async move {
        executor
            .execute(move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, InfrastructureError>(())
            })
            .await
    });
    entered_rx.await.unwrap();

    let listener = ProxyListener {
        id: ListenerId::new(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            request_body_codec: BodyCodecKind::Utf8,
            response_body_codec: BodyCodecKind::ShiftJis,
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let epoch = Uuid::new_v4();
    let resolver = Arc::new(WorkspaceBodyCodecResolver::new());
    resolver.install_listener(epoch, Uuid::new_v4(), &listener);
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        Arc::new(StaticRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::new(Vec::new())),
        }),
        Arc::new(InMemorySessionStore::default()),
        Arc::new(EventHub::new(8)),
        test_capture_repository(),
    )
    .with_body_codec_resolver(resolver);
    let context = test_context(
        epoch,
        Uuid::new_v4(),
        ChannelId::new(listener.id.to_string()).unwrap(),
    );
    let message = request_message("body");
    open_test_connection(&pipeline, &context).await;
    let mut request = message;
    pipeline
        .apply_request_policy(&context, request_metadata(), &mut request)
        .await
        .unwrap();

    let mut response = response_message();
    pipeline
        .apply_response_policy(&context, request_metadata(), &mut response)
        .await
        .unwrap();

    release_tx.send(()).unwrap();
    occupied.await.unwrap().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn request_stage_mock_text_is_encoded_with_the_response_codec() {
    use intercept_proxy_domain::{
        BodyCodecKind, Condition, HttpListenerSettings, HttpRuleContent, ListenerDataPlane,
        ListenerId, MatchField, MatchOperator, ProxyListener, RuleContent, RuleDefinition,
        RuleDefinitionDraft, RuleStage, TerminalAction, UnifiedAction,
    };
    use intercept_proxy_runtime::FaultAction;

    use crate::adapters::WorkspaceBodyCodecResolver;

    let listener_id = ListenerId::from_uuid(Uuid::from_u128(0x7472));
    let listener = ProxyListener {
        id: listener_id,
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            request_body_codec: BodyCodecKind::Utf8,
            response_body_codec: BodyCodecKind::ShiftJis,
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let rule = RuleDefinition::create(
        RuleDefinitionDraft {
            name: "D48 mock".into(),
            enabled: true,
            priority: 1,
            listener_id,
            stage: RuleStage::ProxyToUpstream,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition: Condition::Http {
                    field: MatchField::Method,
                    operator: MatchOperator::Equals("POST".into()),
                },
                action: UnifiedAction::Terminal(TerminalAction::MockResponse {
                        status: 200,
                        headers: Vec::new(),
                        body: "結果D48".into(),
                    }),
            }),
        },
        1,
    )
    .unwrap();
    let epoch = Uuid::new_v4();
    let resolver = Arc::new(WorkspaceBodyCodecResolver::new());
    resolver.install_listener(epoch, Uuid::new_v4(), &listener);
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        Arc::new(StaticRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        }),
        Arc::new(InMemorySessionStore::default()),
        Arc::new(EventHub::new(8)),
        test_capture_repository(),
    )
    .with_body_codec_resolver(resolver);
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    open_test_connection(&pipeline, &context).await;

    let mut request = request_message("request");
    let actions = pipeline
        .apply_request_policy(&context, request_metadata(), &mut request)
        .await
        .unwrap();
    let mock_body = actions
        .iter()
        .find_map(|action| match action {
            FaultAction::MockResponse { body, .. } => Some(body.as_ref()),
            _ => None,
        })
        .expect("MockResponse action");
    let (expected, _, had_errors) = encoding_rs::SHIFT_JIS.encode("結果D48");
    assert!(!had_errors);
    assert_eq!(mock_body, expected.as_ref());
}
