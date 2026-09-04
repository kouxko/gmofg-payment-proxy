use super::*;

#[derive(Debug)]
struct FailingShutdownAndroid {
    owners: Vec<AndroidRuntimeOwnerViewModel>,
    fail_owner_lookup: bool,
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
        if self.fail_owner_lookup {
            return Err(AppError::new(
                "ANDROID_OWNER_LOOKUP_FAILED",
                "owner lookup failed",
            ));
        }
        Ok(self.owners.clone())
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
        owners: ["DEVICE-B", "DEVICE-A"]
            .into_iter()
            .map(|serial| AndroidRuntimeOwnerViewModel {
                serial: serial.into(),
                epoch: Uuid::new_v4(),
                mode: AndroidRuntimeOwnerMode::AdbReverse,
                profile_id: format!("profile-{serial}"),
                state: AndroidRuntimeOwnerState::Active,
                source: AndroidRuntimeOwnerSource::Recovery,
                transition_reason: AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage,
                updated_at: Utc::now(),
            })
            .collect(),
        fail_owner_lookup: false,
        stop_calls: AtomicUsize::new(0),
    });
    let application = application_with_fake_ports_and_android(ports.clone(), android.clone());
    assert_eq!(
        application
            .device_network_runtime_owners()
            .await
            .unwrap()
            .into_iter()
            .map(|owner| owner.serial)
            .collect::<Vec<_>>(),
        ["DEVICE-A", "DEVICE-B"]
    );

    let error = application
        .app_shutdown()
        .await
        .expect_err("owner failure stays visible");

    assert_eq!(error.view_model.code, "APP_SHUTDOWN_FAILED");
    assert!(error.view_model.message.contains("DEVICE-A"));
    assert_eq!(android.stop_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn runtime_owner_query_preserves_port_failure() {
    let android = Arc::new(FailingShutdownAndroid {
        owners: Vec::new(),
        fail_owner_lookup: true,
        stop_calls: AtomicUsize::new(0),
    });
    let application =
        application_with_fake_ports_and_android(Arc::new(FakePorts::default()), android);

    let error = application
        .device_network_runtime_owners()
        .await
        .expect_err("owner lookup failure stays visible");

    assert_eq!(error.view_model.code, "ANDROID_OWNER_LOOKUP_FAILED");
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
        .find(|capability| capability.stage == RuleStage::ProxyToUpstream)
        .expect("request capability");
    let response = capabilities
        .iter()
        .find(|capability| capability.stage == RuleStage::ProxyToApp)
        .expect("response capability");
    assert_eq!(capabilities.len(), 2);
    assert!(!request.actions.iter().any(|action| matches!(
        action.kind,
        RuleActionKind::SetJsonField | RuleActionKind::SetHeader | RuleActionKind::MockResponse
    )));
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
    assert_eq!(request.actions.len(), 11);
    assert_eq!(response.actions.len(), 10);
    assert!(
        !response
            .actions
            .iter()
            .any(|action| action.kind == RuleActionKind::MockResponse)
    );
}
