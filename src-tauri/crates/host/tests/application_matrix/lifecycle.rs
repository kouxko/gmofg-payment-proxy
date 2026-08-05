#[tokio::test]
async fn injected_supervisor_exercises_lifecycle_and_breakpoint_state_without_ui() {
    let epoch = Uuid::from_u128(0x0027_4007_2778);
    let proxy = Arc::new(LifecycleProxy::new(epoch));
    let breakpoints = Arc::new(BreakpointCoordinator::default());
    let temp = tempfile::tempdir().expect("temporary lifecycle host");
    let host = ApplicationHostBuilder::new(temp.path(), test_platform(), Arc::new(TestProfile))
        .with_proxy_supervisor(proxy)
        .with_breakpoint_coordinator(Arc::clone(&breakpoints))
        .build()
        .await
        .expect("build host with deterministic supervisor");
    let application = host.application();

    assert_eq!(
        application
            .proxy_get_status()
            .await
            .expect("initial status")
            .state,
        ProxyState::Stopped
    );
    assert_eq!(
        application.proxy_start().await.expect("start proxy").state,
        ProxyState::Running
    );
    assert_eq!(
        application
            .proxy_restart()
            .await
            .expect("restart proxy")
            .state,
        ProxyState::Running
    );

    let breakpoint_id = Uuid::from_u128(0x48);
    let ticket = breakpoints
        .register(breakpoint_detail(breakpoint_id, epoch))
        .expect("register a pending breakpoint");
    let original = application
        .breakpoint_get(breakpoint_id, epoch)
        .expect("get breakpoint");
    let formatted = application
        .breakpoint_format_json(BreakpointDraft {
            breakpoint_id,
            expected_revision: original.summary.revision,
            message: original.effective.clone(),
        })
        .expect("format JSON in Rust");
    let validation = application
        .breakpoint_validate(&formatted, epoch)
        .expect("validate breakpoint draft");
    assert!(validation.valid);

    let resolved = application
        .breakpoint_resolve(
            epoch,
            BreakpointDecision {
                breakpoint_id,
                expected_revision: original.summary.revision,
                kind: BreakpointDecisionKind::ForwardOriginal,
                message: None,
                delay_ms: None,
                http_status: None,
                content_length_delta: None,
                truncate_at: None,
            },
        )
        .await
        .expect("resolve breakpoint through application");
    assert_eq!(resolved.state, BreakpointState::Resolved);
    assert!(matches!(
        ticket.outcome.await.expect("breakpoint outcome"),
        BreakpointOutcome::Decision(_)
    ));

    assert_eq!(
        application.proxy_stop().await.expect("stop proxy").state,
        ProxyState::Stopped
    );
    host.shutdown().await.expect("shutdown deterministic host");
}

fn message_content() -> MessageContentViewModel {
    let body = br#"{"TransactionType":"0001","RequestID":"R"}"#.to_vec();
    MessageContentViewModel {
        http_status: None,
        start_line_bytes: b"POST / HTTP/1.1".to_vec(),
        raw_headers: Vec::new(),
        headers: BTreeMap::from([
            ("content-type".into(), vec!["application/json".into()]),
            ("content-length".into(), vec![body.len().to_string()]),
        ]),
        body_text: Some(String::from_utf8(body.clone()).expect("ASCII JSON")),
        body_bytes: body,
        json: None,
        content_length: 42,
    }
}

fn breakpoint_detail(breakpoint_id: Uuid, epoch: Uuid) -> BreakpointDetailViewModel {
    BreakpointDetailViewModel {
        summary: BreakpointSummaryViewModel {
            breakpoint_id,
            session_id: Uuid::from_u128(1),
            runtime_epoch: epoch,
            stage: MessageStage::Request,
            title: "请求断点".into(),
            terminal_ip: "10.0.34.94".into(),
            channel: ChannelId::new("beta").unwrap(),
            channel_text: "Beta".into(),
            method: "POST".into(),
            target: "/".into(),
            waiting_since: Utc::now(),
            certificate_fingerprint_suffix: "D4:8".into(),
            state: BreakpointState::Pending,
            state_text: "等待处理".into(),
            ui_tone: UiTone::Warning,
            revision: 1,
        },
        original: message_content(),
        effective: message_content(),
        can_resolve: true,
        resolve_disabled_reason: None,
        available_actions: Vec::new(),
    }
}

fn proxy_status(state: ProxyState, epoch: Uuid) -> ProxyStatusViewModel {
    let (state_text, ui_tone) = state.display_zh();
    ProxyStatusViewModel {
        state,
        state_text: state_text.into(),
        ui_tone,
        runtime_epoch: (state == ProxyState::Running).then_some(epoch),
        revision: 1,
        channels: Vec::new(),
        app_to_proxy_health: ConnectionHealthViewModel {
            state: ConnectionHealthState::Unavailable,
            state_text: "测试替身".into(),
            detail: "无 UI 生命周期测试".into(),
            ui_tone: UiTone::Neutral,
        },
        proxy_to_server_health: ConnectionHealthViewModel {
            state: ConnectionHealthState::Unavailable,
            state_text: "测试替身".into(),
            detail: "无 UI 生命周期测试".into(),
            ui_tone: UiTone::Neutral,
        },
        active_sessions: 0,
        pending_breakpoints: 0,
        logical_memory_bytes: 0,
        logical_memory_text: "0 B".into(),
        memory_capacity_bytes: 256 * 1024 * 1024,
        memory_capacity_text: "256.0 MiB".into(),
        memory_usage_percent: 0,
        session_capacity: 500,
        default_timeout_seconds: 70,
        can_start: state == ProxyState::Stopped,
        start_disabled_reason: None,
        can_stop: state == ProxyState::Running,
        stop_disabled_reason: None,
        can_restart: state == ProxyState::Running,
        restart_disabled_reason: None,
        fault_reason: None,
    }
}
