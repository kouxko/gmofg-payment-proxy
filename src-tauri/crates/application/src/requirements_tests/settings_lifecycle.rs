use super::*;

#[tokio::test]
async fn settings_use_case_accepts_safe_defaults_and_normalizes_before_port_validation() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    // 通用化以后，首次启动的系统设置必须可以独立保存；代理入口、上游地址和证书
    // 已经属于 Workspace Listener，不能再用旧 Payment 固定通道约束把默认值判无效。
    let safe_defaults = SettingsDraft::default();
    let validation = application
        .settings_validate(safe_defaults)
        .await
        .expect("validation result");
    assert!(validation.valid);
    assert_eq!(ports.settings_validations.load(Ordering::SeqCst), 1);

    let mut valid = valid_settings_draft();
    valid.channels[0].upstream_url = " https://alpha.example.test ".into();
    assert!(
        application
            .settings_validate(valid)
            .await
            .expect("fake validation result")
            .valid
    );
    assert_eq!(ports.settings_validations.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn settings_san_raw_input_is_normalized_atomically_in_rust() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    let draft = ports.settings.lock().stored.clone();
    let mut events = application
        .app_subscribe_events(0)
        .expect("subscribe before save");

    let saved = application
        .settings_save_input(draft, " 127.0.0.1，127.0.0.1, ".into())
        .await
        .expect("save normalized settings");

    assert_eq!(saved.stored.leaf_sans, vec!["127.0.0.1"]);
    let event = events.live.recv().await.expect("settings changed event");
    assert_eq!(event.entity_id.as_deref(), Some("settings"));
    assert_eq!(event.entity_revision, Some(saved.revision));
    let UiEventPayload::SettingsChanged(changed) = event.payload else {
        panic!("expected SettingsChanged");
    };
    assert_eq!(changed.stored.leaf_sans, vec!["127.0.0.1"]);
}

#[tokio::test]
async fn settings_can_be_saved_without_listener_certificate_warnings() {
    let ports = Arc::new(FakePorts::default());
    {
        let mut overview = ports.certificate_overview.lock();
        overview.ready = false;
        overview.items.clear();
        overview.can_initialize = true;
        overview.status_text = "证书配置不完整".into();
        overview.ui_tone = UiTone::Warning;
    }
    let application = application_with_fake_ports(ports);
    let draft = SettingsDraft {
        leaf_sans: vec!["10.0.34.50".into()],
        ..valid_settings_draft()
    };

    let validation = application.settings_validate(draft).await.unwrap();

    assert!(validation.valid);
    // Listener 在各自的 Workspace 中校验 TLS 身份。系统设置不能重复推断证书状态，
    // 否则一个无关 Listener 尚未配置也会污染全局容量或超时设置的保存结果。
    assert!(validation.warnings.is_empty());
}

#[tokio::test]
async fn settings_validation_matches_prefixed_certificate_sans() {
    let ports = Arc::new(FakePorts::default());
    ports.certificate_overview.lock().items[0].sans =
        vec!["IP:10.0.34.50".into(), "DNS:Proxy.Local".into()];
    let application = application_with_fake_ports(ports);
    let draft = SettingsDraft {
        leaf_sans: vec!["10.0.34.50".into(), "proxy.local".into()],
        ..valid_settings_draft()
    };

    let validation = application.settings_validate(draft).await.unwrap();

    assert!(validation.valid);
    assert!(!validation.field_errors.contains_key("leaf_sans"));
}

#[tokio::test]
async fn empty_pkcs12_password_is_forwarded_to_the_rust_certificate_parser() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);

    let overview = application.certificate_import_pkcs12(String::new()).await;

    assert!(overview.is_ok());
}

#[tokio::test]
async fn settings_restart_preserves_candidate_error_after_successful_rollback() {
    let ports = Arc::new(FakePorts::default());
    *ports.proxy_state.lock() = ProxyState::Running;
    ports.start_results.lock().extend([
        Err(AppError::new("PORT_IN_USE", "候选端口已被占用。")),
        Ok(proxy_status(ProxyState::Running)),
    ]);
    let original = ports.settings.lock().clone();
    let application = application_with_fake_ports(ports.clone());
    let mut candidate = original.stored.clone();
    candidate.channels[0].port = 20_003;

    let error = application
        .settings_save_and_restart(candidate)
        .await
        .expect_err("candidate must fail and roll back");

    assert_eq!(error.view_model.code, "PORT_IN_USE");
    assert!(error.view_model.message.contains("已恢复"));
    assert_eq!(ports.start_calls.load(Ordering::SeqCst), 2);
    assert_eq!(*ports.proxy_state.lock(), ProxyState::Running);
    let restored = ports.settings.lock();
    assert_eq!(restored.stored, original.stored);
    assert_eq!(restored.effective, original.effective);
}

#[tokio::test]
async fn starting_and_stopping_block_every_rule_and_fault_write() {
    for state in [ProxyState::Starting, ProxyState::Stopping] {
        let ports = Arc::new(FakePorts::default());
        *ports.proxy_state.lock() = state;
        let application = application_with_fake_ports(ports);
        let draft = application
            .rule_new_draft()
            .await
            .expect("draft is read-only");
        assert_eq!(
            application
                .rule_save(draft)
                .await
                .expect_err("rule save is gated")
                .view_model
                .code,
            "OPERATION_IN_PROGRESS"
        );
        assert_eq!(
            application
                .rule_toggle(Uuid::new_v4(), 1, false)
                .await
                .expect_err("rule toggle is gated")
                .view_model
                .code,
            "OPERATION_IN_PROGRESS"
        );
        assert_eq!(
            application
                .fault_configure(FaultConfigurationDraft {
                    template_id: "delay".into(),
                    existing_rule_id: None,
                    expected_revision: None,
                    channel: None,
                    terminal: None,
                    target: None,
                    nth_hit: None,
                    one_shot: false,
                    priority: 1,
                    parameters: BTreeMap::new(),
                })
                .await
                .expect_err("fault configure is gated")
                .view_model
                .code,
            "OPERATION_IN_PROGRESS"
        );
    }
}

#[tokio::test]
async fn lifecycle_mutations_serialize_settings_and_certificate_writes() {
    let ports = Arc::new(FakePorts::default());
    ports.block_start.store(true, Ordering::SeqCst);
    let application = Arc::new(application_with_fake_ports(ports.clone()));

    let starting = {
        let application = application.clone();
        tokio::spawn(async move { application.proxy_start().await })
    };
    ports.start_entered.notified().await;

    let draft = ports.settings.lock().stored.clone();
    let mut saving = {
        let application = application.clone();
        tokio::spawn(async move { application.settings_save(draft).await })
    };
    let mut importing = {
        let application = application.clone();
        tokio::spawn(async move { application.certificate_import_pkcs12(String::new()).await })
    };

    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut saving)
            .await
            .is_err(),
        "settings save must wait for the lifecycle mutation"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut importing)
            .await
            .is_err(),
        "certificate import must wait for the lifecycle mutation"
    );

    ports.continue_start.notify_one();
    assert_eq!(
        starting.await.expect("start task").expect("start").state,
        ProxyState::Running
    );
    saving
        .await
        .expect("settings task")
        .expect("settings remain writable while running");
    let import_error = importing
        .await
        .expect("certificate task")
        .expect_err("certificate mutation requires a stopped proxy");

    assert_eq!(import_error.view_model.code, "OPERATION_IN_PROGRESS");
    assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.certificate_import_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn application_shutdown_stops_runtime_clears_effective_settings_and_is_idempotent() {
    let ports = Arc::new(FakePorts::default());
    *ports.proxy_state.lock() = ProxyState::Running;
    let application = application_with_fake_ports(ports.clone());

    let stopped = application.app_shutdown().await.expect("shutdown");
    assert_eq!(stopped.state, ProxyState::Stopped);
    assert_eq!(ports.stop_calls.load(Ordering::SeqCst), 1);
    assert!(ports.settings.lock().effective.is_none());

    let stopped_again = application
        .app_shutdown()
        .await
        .expect("idempotent shutdown");
    assert_eq!(stopped_again.state, ProxyState::Stopped);
    assert_eq!(ports.stop_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn application_shutdown_stops_every_dynamic_workspace_listener() {
    let ports = Arc::new(FakePorts::default());
    let listener_runtime = Arc::new(InMemoryListenerRuntime::default());
    let workspace = ProxyWorkspace::default();
    let listener = workspace
        .listeners
        .clone()
        .into_iter()
        .next()
        .expect("default forward listener");
    listener_runtime
        .start(workspace, listener)
        .await
        .expect("listener starts before shutdown");
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces: Arc::new(InMemoryWorkspaceStore::default()),
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: listener_runtime.clone(),
            events: Arc::new(EventHub::default()),
        },
    );

    application.app_shutdown().await.expect("shutdown");

    assert!(
        listener_runtime.statuses().await.unwrap().is_empty(),
        "application shutdown must not leave dynamic listener tasks running"
    );
}

#[test]
fn rule_editor_primitives_and_byte_parser_are_owned_by_rust() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    assert_eq!(
        application.rule_condition_draft(RuleConditionKind::NthHit),
        RuleCondition::NthHit { count: 1 }
    );
    assert!(matches!(
        application.rule_action_draft(RuleActionKind::MockResponse),
        RuleAction::Terminal {
            action: RuleTerminalAction::MockResponse { .. }
        }
    ));
    assert_eq!(
        application.rule_match_field_draft(RuleMatchFieldKind::JsonPath),
        RuleMatchField::JsonPath {
            path: "$.field".into()
        }
    );
    assert_eq!(
        application.rule_match_operator_draft(RuleMatchOperatorKind::Regex),
        RuleMatchOperator::Regex {
            pattern: String::new()
        }
    );
    assert_eq!(
        application
            .rule_parse_byte_input(" 123, 0,255 ")
            .expect("valid bytes"),
        RuleByteInputViewModel {
            bytes: vec![123, 0, 255],
            normalized: "123, 0, 255".into(),
        }
    );
    let error = application
        .rule_parse_byte_input("1, 256")
        .expect_err("out of range");
    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert!(error.view_model.field_errors.contains_key("raw"));
    assert_eq!(
        application
            .rule_parse_header_input(" Content-Type: application/json \nX-Trace: abc:123 ")
            .expect("valid headers"),
        RuleHeaderInputViewModel {
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("x-trace".into(), "abc:123".into()),
            ],
            normalized: "content-type: application/json\nx-trace: abc:123".into(),
        }
    );
    let error = application
        .rule_parse_header_input("missing-separator")
        .expect_err("invalid header line");
    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert!(error.view_model.field_errors.contains_key("raw"));
}
