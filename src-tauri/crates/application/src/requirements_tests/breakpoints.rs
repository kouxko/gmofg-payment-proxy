use super::*;

#[test]
fn breakpoint_action_view_model_is_stage_specific_and_rust_owned() {
    let coordinator = BreakpointCoordinator::default();
    let epoch = Uuid::new_v4();
    let request = coordinator
        .register(breakpoint(Uuid::new_v4(), epoch, 1))
        .expect("request breakpoint");
    let request_kinds = request
        .detail
        .available_actions
        .iter()
        .map(|action| action.kind)
        .collect::<Vec<_>>();
    assert!(request_kinds.contains(&BreakpointDecisionKind::MockResponse));
    assert!(request_kinds.contains(&BreakpointDecisionKind::DisconnectBeforeUpstream));
    assert!(!request_kinds.contains(&BreakpointDecisionKind::CustomHttpStatus));

    let mut response_detail = breakpoint(Uuid::new_v4(), epoch, 2);
    response_detail.summary.stage = MessageStage::Response;
    let response = coordinator
        .register(response_detail)
        .expect("response breakpoint");
    let response_kinds = response
        .detail
        .available_actions
        .iter()
        .map(|action| action.kind)
        .collect::<Vec<_>>();
    assert!(!response_kinds.contains(&BreakpointDecisionKind::MockResponse));
    assert!(!response_kinds.contains(&BreakpointDecisionKind::DisconnectBeforeUpstream));
    assert!(response_kinds.contains(&BreakpointDecisionKind::CustomHttpStatus));
}

#[tokio::test]
async fn breakpoint_resolution_is_atomic_and_epoch_scoped() {
    let coordinator = BreakpointCoordinator::default();
    let id = Uuid::from_u128(10);
    let epoch = Uuid::from_u128(20);
    let ticket = coordinator
        .register(breakpoint(id, epoch, 1))
        .expect("register");
    assert_eq!(ticket.detail.summary.state_text, "等待处理");
    let decision = BreakpointDecision {
        breakpoint_id: id,
        expected_revision: 7,
        kind: BreakpointDecisionKind::ForwardOriginal,
        message: None,
        delay_ms: None,
        http_status: None,
        content_length_delta: None,
        truncate_at: None,
    };
    let summary = coordinator
        .resolve(epoch, decision.clone())
        .expect("first resolution");
    assert_eq!(summary.state, BreakpointState::Resolved);
    assert!(matches!(
        ticket.outcome.await.expect("outcome delivered"),
        BreakpointOutcome::Decision(decision)
            if decision.kind == BreakpointDecisionKind::ForwardOriginal
    ));
    assert_eq!(
        coordinator
            .resolve(epoch, decision)
            .expect_err("second resolution fails")
            .view_model
            .code,
        "BREAKPOINT_ALREADY_RESOLVED"
    );
}

// BREAKPOINT-013~015: cancellation delivers the exact terminal cause to the waiting runtime.
#[tokio::test]
async fn breakpoint_cancellation_preserves_client_and_proxy_causes() {
    let coordinator = BreakpointCoordinator::default();
    let epoch = Uuid::from_u128(20);

    let client_id = Uuid::from_u128(11);
    let client_ticket = coordinator
        .register(breakpoint(client_id, epoch, 1))
        .expect("register client breakpoint");
    let client = coordinator
        .client_disconnected(client_id)
        .expect("terminate disconnected client");
    assert_eq!(client.state, BreakpointState::ClientDisconnected);
    assert_eq!(
        client_ticket.outcome.await.expect("client outcome"),
        BreakpointOutcome::ClientDisconnected
    );

    let stop_id = Uuid::from_u128(12);
    let stop_ticket = coordinator
        .register(breakpoint(stop_id, epoch, 2))
        .expect("register stop breakpoint");
    let stopped = coordinator.proxy_stopped(epoch);
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0].state, BreakpointState::ProxyStopped);
    assert_eq!(
        stop_ticket.outcome.await.expect("stop outcome"),
        BreakpointOutcome::ProxyStopped
    );
}

// DATA-005: terminal tombstones are bounded and old identifiers expire deterministically.
#[test]
fn breakpoint_terminal_tombstones_are_bounded() {
    let coordinator = BreakpointCoordinator::default();
    let epoch = Uuid::from_u128(20);
    let first = Uuid::from_u128(1);
    for index in 1..=4_097_u128 {
        let id = Uuid::from_u128(index);
        coordinator
            .register(breakpoint(id, epoch, 1))
            .expect("register");
        coordinator
            .client_disconnected(id)
            .expect("terminate breakpoint");
    }
    assert_eq!(
        coordinator
            .client_disconnected(first)
            .expect_err("old tombstone expired")
            .view_model
            .code,
        "BREAKPOINT_NOT_FOUND"
    );
    assert_eq!(
        coordinator
            .client_disconnected(Uuid::from_u128(4_097))
            .expect_err("new tombstone retained")
            .view_model
            .code,
        "BREAKPOINT_CLIENT_DISCONNECTED"
    );
}

// TEST-EVENT, NFR-004: capture events flush at 200 rows or 100 ms.
#[tokio::test]
async fn breakpoint_resolve_normalizes_modified_json_inside_rust_use_case() {
    let ports = Arc::new(FakePorts::default());
    *ports.proxy_state.lock() = ProxyState::Running;
    let coordinator = Arc::new(BreakpointCoordinator::default());
    let epoch = Uuid::from_u128(20);
    let id = Uuid::from_u128(30);
    let ticket = coordinator
        .register(breakpoint(id, epoch, 1))
        .expect("register");
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: coordinator,
            breakpoint_validation: Arc::new(breakpoint_validator()),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            workspaces: Arc::new(InMemoryWorkspaceStore::default()),
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: Arc::new(InMemoryListenerRuntime::default()),
            events: Arc::new(EventHub::default()),
        },
    );

    application
        .breakpoint_resolve(
            epoch,
            BreakpointDecision {
                breakpoint_id: id,
                expected_revision: 7,
                kind: BreakpointDecisionKind::ForwardModified,
                message: Some(MessageContentViewModel {
                    http_status: None,
                    start_line_bytes: Vec::new(),
                    raw_headers: Vec::new(),
                    headers: BTreeMap::from([(
                        "content-type".into(),
                        vec!["application/json".into()],
                    )]),
                    body_text: Some(r#"{"amount":100}"#.into()),
                    body_bytes: b"stale".to_vec(),
                    json: None,
                    content_length: 5,
                    media_type: Some("application/json".into()),
                    charset: None,
                    content_kind: MessageContentKind::Json,
                    codec_id: Some("utf-8".into()),
                    decode_error: None,
                    query_string: None,
                }),
                delay_ms: Some(1_000),
                http_status: Some(503),
                content_length_delta: Some(1),
                truncate_at: Some(1),
            },
        )
        .await
        .expect("resolve");

    let BreakpointOutcome::Decision(decision) = ticket.outcome.await.expect("outcome") else {
        panic!("expected decision");
    };
    let message = decision.message.expect("normalized message");
    assert_eq!(
        message.body_text.as_deref(),
        Some("{\n  \"amount\": 100\n}")
    );
    assert_eq!(message.content_length, message.body_bytes.len());
    assert_eq!(
        message.headers.get("content-length"),
        Some(&vec![message.content_length.to_string()])
    );
}

// SETTINGS-001~012, TEST-SETTINGS, TEST-IPC: facade normalizes and validates before fake storage.
