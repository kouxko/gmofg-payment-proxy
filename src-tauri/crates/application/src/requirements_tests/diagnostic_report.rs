use super::*;

#[derive(Debug)]
struct DiagnosticAndroidPort {
    owners: Vec<AndroidRuntimeOwnerViewModel>,
}

#[async_trait]
impl AndroidControlPort for DiagnosticAndroidPort {
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
        unused()
    }
    async fn emergency_restore(
        &self,
        _: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_status(
        &self,
        target: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let owner = self
            .owners
            .iter()
            .find(|owner| owner.serial == target.serial)
            .unwrap();
        if owner.serial == "DEVICE-A" {
            return Err(AppError::new("ANDROID_STATUS_FAILED", "status failed"));
        }
        Ok(AndroidNetworkStatusViewModel {
            serial: owner.serial.clone(),
            runtime_epoch: Some(owner.epoch),
            state: AndroidNetworkState::Running,
            state_text: "运行中".into(),
            ui_tone: UiTone::Positive,
            verified: true,
            transport: AndroidControlTransport::LocalAbstractSocket,
            active_profile_id: Some(owner.profile_id.clone()),
            active_profile_fingerprint: None,
            active_route_fingerprint: None,
            active_route_count: 0,
            companion_process_running: Some(true),
            message: "running".into(),
            unsupported_fields: Vec::new(),
            stats: None,
        })
    }
    async fn runtime_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        Ok(self.owners.clone())
    }
    async fn network_runtime_endpoints(
        &self,
        target: AndroidDeviceTarget,
        _: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
        if target.serial == "DEVICE-A" {
            return Err(AppError::new(
                "ANDROID_ENDPOINTS_FAILED",
                "endpoints failed",
            ));
        }
        Ok(Vec::new())
    }
}

fn diagnostic_owner(serial: &str) -> AndroidRuntimeOwnerViewModel {
    AndroidRuntimeOwnerViewModel {
        serial: serial.into(),
        epoch: Uuid::new_v4(),
        mode: AndroidRuntimeOwnerMode::DeviceOnly,
        profile_id: format!("profile-{serial}"),
        state: AndroidRuntimeOwnerState::Active,
        source: AndroidRuntimeOwnerSource::Recovery,
        transition_reason: AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage,
        updated_at: Utc::now(),
    }
}

fn record_listener_diagnostics(application: &Application, listener_id: ListenerId) {
    for index in 0..150 {
        application.diagnostic_log_record(DiagnosticLogEntryViewModel {
            level: DiagnosticLogLevel::Info,
            stage: DiagnosticLogStage::Socket,
            summary: format!("listener evidence {index}"),
            detail: None,
            device_serial: None,
            listener_id: Some(listener_id.to_string()),
            profile_id: None,
            socket_context: None,
        });
    }
    application.diagnostic_log_record(DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Error,
        stage: DiagnosticLogStage::Socket,
        summary: "another listener".into(),
        detail: None,
        device_serial: None,
        listener_id: Some(ListenerId::new().to_string()),
        profile_id: None,
        socket_context: None,
    });
}

#[tokio::test]
async fn diagnostic_report_aggregates_bounded_listener_evidence_and_markdown() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let application = application_with_workspace_ports(Arc::clone(&ports), Arc::clone(&workspaces));
    let workspace = workspaces
        .list()
        .await
        .expect("workspace summaries")
        .into_iter()
        .next()
        .expect("default workspace");
    let listener = workspaces
        .get(workspace.id)
        .await
        .expect("workspace")
        .listeners
        .into_iter()
        .next()
        .expect("default listener");

    record_listener_diagnostics(&application, listener.id);

    let report = application
        .diagnostic_report_generate(DiagnosticReportQuery {
            workspace_id: workspace.id,
            listener_id: listener.id,
        })
        .await
        .expect("diagnostic report");

    assert_eq!(report.bundle.workspace.id, workspace.id);
    assert_eq!(report.bundle.listener.id, listener.id);
    assert_eq!(
        report
            .bundle
            .runtime_status
            .as_ref()
            .map(|status| status.state.clone()),
        Some(ListenerRuntimeState::Stopped)
    );
    assert!(report.bundle.settings.is_some());
    assert!(report.bundle.external_package_service.is_some());
    assert!(report.bundle.protocol_rules.is_empty());
    assert!(report.bundle.protocol_package_detail.is_none());
    assert_eq!(
        report.bundle.diagnostics.len(),
        DIAGNOSTIC_REPORT_MAX_DIAGNOSTICS
    );
    assert!(
        report
            .bundle
            .diagnostics
            .iter()
            .all(|row| { row.listener_id.as_deref() == Some(listener.id.to_string().as_str()) })
    );
    assert!(report.bundle.android_network_statuses.is_empty());
    assert!(report.bundle.android_runtime_owners.is_empty());
    assert!(report.bundle.android_runtime_endpoints.is_empty());
    assert!(
        report
            .bundle
            .collection_errors
            .iter()
            .all(|error| { error.section != DiagnosticReportSection::AndroidNetworkStatus }),
        "没有保留运行所有者时不得虚构一个依赖全局设备选择的 Android 状态查询"
    );
    assert!(
        report
            .bundle
            .environment
            .architecture_refs
            .iter()
            .any(|reference| { reference.contains("application") })
    );
    assert!(report.markdown.contains(&workspace.id.to_string()));
    assert!(report.markdown.contains(&listener.id.to_string()));
    assert!(report.markdown.contains("复现步骤"));
    assert!(report.markdown.contains("数据平面：HTTP"));
    assert!(report.markdown.contains("网络拓扑：HTTP proxy"));
    assert!(
        report
            .markdown
            .contains("转发方式：按客户端请求目标动态转发")
    );
    assert!(report.markdown.chars().count() <= DIAGNOSTIC_REPORT_MARKDOWN_MAX_CHARS);
}

#[tokio::test]
async fn diagnostic_android_errors_keep_owner_serial_and_epoch() {
    let ports = Arc::new(FakePorts::default());
    let owner_a = diagnostic_owner("DEVICE-A");
    let owner_b = diagnostic_owner("DEVICE-B");
    let application = application_with_fake_ports_and_android(
        Arc::clone(&ports),
        Arc::new(DiagnosticAndroidPort {
            owners: vec![owner_a.clone(), owner_b],
        }),
    );
    let workspace = application.workspace_list().await.unwrap()[0].clone();
    let listener = application
        .workspace_get(workspace.id)
        .await
        .unwrap()
        .listeners[0]
        .clone();

    let report = application
        .diagnostic_report_generate(DiagnosticReportQuery {
            workspace_id: workspace.id,
            listener_id: listener.id,
        })
        .await
        .unwrap();

    let android_errors = report
        .bundle
        .collection_errors
        .iter()
        .filter(|error| {
            matches!(
                error.section,
                DiagnosticReportSection::AndroidNetworkStatus
                    | DiagnosticReportSection::AndroidRuntimeEndpoints
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(android_errors.len(), 2);
    assert!(android_errors.iter().all(|error| {
        error.entity_id.as_deref() == Some("DEVICE-A") && error.runtime_epoch == Some(owner_a.epoch)
    }));
    assert_eq!(report.bundle.android_network_statuses.len(), 1);
    assert_eq!(report.bundle.android_network_statuses[0].serial, "DEVICE-B");
}

#[tokio::test]
async fn diagnostic_report_rejects_listener_outside_requested_workspace() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let application = application_with_workspace_ports(ports, Arc::clone(&workspaces));
    let workspace_id = workspaces.list().await.expect("workspace summaries")[0].id;

    let error = application
        .diagnostic_report_generate(DiagnosticReportQuery {
            workspace_id,
            listener_id: ListenerId::new(),
        })
        .await
        .expect_err("foreign listener must fail");

    assert_eq!(error.view_model.code, "LISTENER_NOT_FOUND");
    assert_eq!(error.view_model.entity_id, Some(workspace_id.to_string()));
}

#[tokio::test]
async fn diagnostic_report_filters_listener_diagnostics_before_applying_its_limit() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let application = application_with_workspace_ports(ports, Arc::clone(&workspaces));
    let workspace_id = workspaces.list().await.expect("workspace summaries")[0].id;
    let listener_id = workspaces
        .get(workspace_id)
        .await
        .expect("workspace")
        .listeners[0]
        .id;
    application.diagnostic_log_record(DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Error,
        stage: DiagnosticLogStage::Socket,
        summary: "target listener evidence".into(),
        detail: None,
        device_serial: None,
        listener_id: Some(listener_id.to_string()),
        profile_id: None,
        socket_context: None,
    });
    for index in 0..550 {
        application.diagnostic_log_record(DiagnosticLogEntryViewModel {
            level: DiagnosticLogLevel::Info,
            stage: DiagnosticLogStage::Socket,
            summary: format!("unrelated listener evidence {index}"),
            detail: None,
            device_serial: None,
            listener_id: Some(ListenerId::new().to_string()),
            profile_id: None,
            socket_context: None,
        });
    }

    let report = application
        .diagnostic_report_generate(DiagnosticReportQuery {
            workspace_id,
            listener_id,
        })
        .await
        .expect("diagnostic report");

    assert_eq!(report.bundle.diagnostics.len(), 1);
    assert_eq!(
        report.bundle.diagnostics[0].summary,
        "target listener evidence"
    );
}

#[tokio::test]
async fn diagnostic_report_keeps_exact_package_binding_when_detail_is_unavailable() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let application = application_with_workspace_ports(Arc::clone(&ports), Arc::clone(&workspaces));
    let workspace_id = workspaces.list().await.expect("workspace summaries")[0].id;
    let mut workspace = workspaces.get(workspace_id).await.expect("workspace");
    let listener = workspace.listeners.first_mut().expect("default listener");
    let listener_id = listener.id;
    let package = protocol_package("missing-external", "1.2.3");
    let mut socket = SocketRelaySettings {
        processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: package.clone(),
        }),
        ..SocketRelaySettings::default()
    };
    let SocketTopology::Relay(relay) = &mut socket.topology else {
        panic!("default Socket topology must relay")
    };
    relay.upstream = SocketEndpoint {
        host: "127.0.0.1".into(),
        port: 9_999,
    };
    listener.data_plane = ListenerDataPlane::Socket(socket);
    workspaces.save(workspace).await.expect("save workspace");

    let report = application
        .diagnostic_report_generate(DiagnosticReportQuery {
            workspace_id,
            listener_id,
        })
        .await
        .expect("partial report remains available");

    assert!(report.bundle.protocol_package_detail.is_none());
    assert!(report.bundle.collection_errors.iter().any(|error| {
        error.section == DiagnosticReportSection::ProtocolPackageDetail
            && error.code == "PROTOCOL_PACKAGE_NOT_FOUND"
    }));
    assert!(report.markdown.contains("missing-external@1.2.3"));
}
