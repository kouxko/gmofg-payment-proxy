use super::*;

#[derive(Debug)]
struct FailingShutdownAndroid {
    owner: AndroidRuntimeOwnerViewModel,
    stop_calls: AtomicUsize,
}

#[async_trait]
impl AndroidControlPort for FailingShutdownAndroid {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        unused()
    }
    async fn adb_select(&self, _: String) -> AppResult<AndroidAdbViewModel> {
        unused()
    }
    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        unused()
    }
    async fn package_list(
        &self,
        _: AndroidDeviceTarget,
    ) -> AppResult<Vec<AndroidPackageViewModel>> {
        unused()
    }
    async fn package_get(
        &self,
        _: AndroidDeviceTarget,
        _: String,
    ) -> AppResult<AndroidPackageViewModel> {
        unused()
    }
    async fn companion_install(
        &self,
        _: AndroidDeviceTarget,
        _: bool,
    ) -> AppResult<AndroidCompanionInstallViewModel> {
        unused()
    }
    async fn vpn_open_consent(
        &self,
        _: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_start(
        &self,
        _: AndroidDeviceTarget,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_apply(
        &self,
        _: AndroidRuntimeTarget,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_runtime_ready(
        &self,
        _: AndroidDeviceTarget,
        _: &AndroidNetworkActivation,
        _: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool> {
        unused()
    }
    async fn network_stop(
        &self,
        _: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        self.stop_calls.fetch_add(1, Ordering::SeqCst);
        Err(AppError::new("ANDROID_OWNER_OFFLINE", "owner offline"))
    }
    async fn emergency_restore(
        &self,
        _: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_status(
        &self,
        _: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn runtime_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        Ok(vec![self.owner.clone()])
    }
    async fn network_runtime_endpoints(
        &self,
        _: AndroidDeviceTarget,
        _: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
        Ok(Vec::new())
    }
}

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
async fn external_package_service_settings_are_normalized_and_validated_before_persistence() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    let mut draft = valid_settings_draft();
    draft.external_package_service.bind_address = " 127.0.0.1 ".into();
    draft.external_package_service.rpc_timeout_seconds = 0;
    draft.external_package_service.max_in_flight = 0;

    let validation = application
        .settings_validate(draft)
        .await
        .expect("external package settings validation result");

    assert!(!validation.valid);
    assert!(
        validation
            .field_errors
            .contains_key("external_package_service.rpc_timeout_seconds")
    );
    assert!(
        validation
            .field_errors
            .contains_key("external_package_service.max_in_flight")
    );
    assert_eq!(
        ports.settings_validations.load(Ordering::SeqCst),
        0,
        "invalid local settings must not reach persistence"
    );
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
async fn application_shutdown_reports_owner_stop_failure_but_completes_other_cleanup() {
    let ports = Arc::new(FakePorts::default());
    let android = Arc::new(FailingShutdownAndroid {
        owner: AndroidRuntimeOwnerViewModel {
            serial: "DEVICE-A".into(),
            epoch: Uuid::new_v4(),
            mode: AndroidRuntimeOwnerMode::AdbReverse,
            profile_id: "profile-a".into(),
            state: AndroidRuntimeOwnerState::Active,
            source: AndroidRuntimeOwnerSource::Recovery,
            transition_reason: AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage,
            updated_at: Utc::now(),
        },
        stop_calls: AtomicUsize::new(0),
    });
    let application = application_with_fake_ports_and_android(ports.clone(), android.clone());
    assert_eq!(
        application
            .device_network_runtime_owners()
            .await
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .serial,
        "DEVICE-A"
    );

    let error = application
        .app_shutdown()
        .await
        .expect_err("owner failure stays visible");

    assert_eq!(error.view_model.code, "APP_SHUTDOWN_FAILED");
    assert!(error.view_model.message.contains("DEVICE-A"));
    assert_eq!(android.stop_calls.load(Ordering::SeqCst), 1);
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
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            workspaces: Arc::new(InMemoryWorkspaceStore::default()),
            listener_runtime: listener_runtime.clone(),
            protocol_packages: unused_protocol_package_services(),
            events: Arc::new(EventHub::default()),
            environment_baseline_capture: test_environment_baseline_capture(),
            environment_identity_allocator: test_environment_identity_allocator(),
            environment_apply_lease: test_environment_apply_lease(),
            environment_material_preparer: test_environment_material_preparer(),
            environment_commit: test_environment_commit(),
            environment_validator: test_environment_validator(),
        },
        Arc::new(UnusedAndroidControlPort),
        Arc::new(UnusedProtectedSecretPort),
    );

    application.app_shutdown().await.expect("shutdown");

    assert!(
        listener_runtime.statuses().await.unwrap().is_empty(),
        "application shutdown must not leave dynamic listener tasks running"
    );
}

#[test]
fn rule_editor_capabilities_are_stage_exact_and_rust_owned() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let capabilities = application.rule_capabilities();
    let request = capabilities
        .iter()
        .find(|capability| capability.stage == MessageStage::Request)
        .expect("request capability");
    let response = capabilities
        .iter()
        .find(|capability| capability.stage == MessageStage::Response)
        .expect("response capability");
    let tls = capabilities
        .iter()
        .find(|capability| capability.stage == MessageStage::TlsHandshake)
        .expect("TLS capability");
    assert!(
        request
            .actions
            .iter()
            .any(|action| action.kind == RuleActionKind::MockResponse)
    );
    assert!(
        !request
            .actions
            .iter()
            .any(|action| action.kind == RuleActionKind::CustomHttpStatus)
    );
    assert!(
        response
            .actions
            .iter()
            .any(|action| action.kind == RuleActionKind::CustomHttpStatus)
    );
    assert!(
        !response
            .actions
            .iter()
            .any(|action| action.kind == RuleActionKind::MockResponse)
    );
    assert_eq!(
        tls.match_field_kinds,
        vec![RuleMatchFieldKind::CertificateFingerprint]
    );
    assert_eq!(tls.actions.len(), 1);
    assert_eq!(tls.actions[0].kind, RuleActionKind::RejectTlsHandshake);
    for stage in &capabilities {
        for action in &stage.actions {
            let draft = application
                .rule_action_draft(action.kind, stage.stage)
                .expect("every advertised action must produce a valid draft");
            match (draft, action.traffic_direction) {
                (
                    RuleAction::Throttle { direction, .. }
                    | RuleAction::Intermittent { direction, .. },
                    Some(expected),
                ) => assert_eq!(direction, expected),
                (RuleAction::Throttle { .. } | RuleAction::Intermittent { .. }, None) => {
                    panic!("directional action must advertise its fixed direction")
                }
                (_, _) => {}
            }
        }
    }
    assert_eq!(
        application
            .rule_condition_draft(RuleConditionKind::NthHit, MessageStage::Request)
            .expect("request condition draft"),
        RuleCondition::NthHit { count: 1 }
    );
    assert!(matches!(
        application
            .rule_action_draft(RuleActionKind::MockResponse, MessageStage::Request)
            .expect("request action draft"),
        RuleAction::Terminal {
            action: RuleTerminalAction::MockResponse { .. }
        }
    ));
    assert_eq!(
        application
            .rule_match_field_draft(RuleMatchFieldKind::JsonPath, MessageStage::Response)
            .expect("response field draft"),
        RuleMatchField::JsonPath {
            path: "$.field".into()
        }
    );
    assert!(
        application
            .rule_action_draft(RuleActionKind::CustomHttpStatus, MessageStage::Request,)
            .is_err()
    );
    assert!(
        application
            .rule_match_field_draft(RuleMatchFieldKind::JsonPath, MessageStage::TlsHandshake,)
            .is_err()
    );
}

#[test]
fn rule_editor_primitives_and_byte_parser_are_owned_by_rust() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
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
