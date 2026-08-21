use super::*;

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

fn capture_detail(
    capture_id: SocketCaptureId,
    workspace_id: WorkspaceId,
    listener_id: ListenerId,
) -> SocketCaptureDetailViewModel {
    let connection_id = SocketConnectionId::new();
    SocketCaptureDetailViewModel {
        record: SocketCaptureRecord {
            capture_id,
            runtime_epoch: Uuid::new_v4(),
            workspace_id,
            listener_id,
            session_id: connection_id.as_uuid(),
            connection_id,
            peer_address: "127.0.0.1:43100".into(),
            occurred_at: Utc::now(),
            completed_at: Utc::now(),
            payload: SocketCapturePayload::RelayFrame(Box::new(SocketRelayFrameCapture {
                direction: ProtocolDirection::Upstream,
                package: protocol_package("report-test", "1.0.0"),
                schema: SocketCaptureSchemaRef {
                    id: DocumentSchemaId::new("request").expect("schema id"),
                    version: 1,
                },
                origin: vec![0x30, 0x32, 0x30, 0x30],
                stages: Vec::new(),
                written: vec![0x30, 0x32, 0x30, 0x30],
                display: SocketDisplayResult::HexFallback {
                    reason: SocketDisplayFallbackReason::EntryPointFailed,
                    diagnostic: None,
                },
            })),
        },
    }
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

    let missing_capture = SocketCaptureId::new();
    let report = application
        .diagnostic_report_generate(DiagnosticReportQuery {
            workspace_id: workspace.id,
            listener_id: listener.id,
            capture_id: Some(missing_capture),
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
    assert_eq!(
        report.bundle.socket_captures.page_size,
        DIAGNOSTIC_REPORT_MAX_CAPTURES
    );
    assert!(report.bundle.capture_detail.is_none());
    assert!(report.bundle.android_network_status.is_none());
    assert!(report.bundle.android_runtime_owner.is_none());
    assert!(report.bundle.android_runtime_endpoints.is_empty());
    assert!(report.bundle.collection_errors.iter().any(|error| {
        error.section == DiagnosticReportSection::CaptureDetail && error.code == "UNUSED_FAKE_PORT"
    }));
    assert!(report.bundle.collection_errors.iter().any(|error| {
        error.section == DiagnosticReportSection::AndroidNetworkStatus
            && error.code == "ANDROID_DEVICE_NOT_SELECTED"
    }));
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

    let queries = ports.socket_capture_queries.lock();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].workspace_id, Some(workspace.id));
    assert_eq!(queries[0].listener_id, Some(listener.id));
    assert_eq!(queries[0].page.page_size, DIAGNOSTIC_REPORT_MAX_CAPTURES);
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
            capture_id: None,
        })
        .await
        .expect_err("foreign listener must fail");

    assert_eq!(error.view_model.code, "LISTENER_NOT_FOUND");
    assert_eq!(error.view_model.entity_id, Some(workspace_id.to_string()));
}

#[tokio::test]
async fn diagnostic_report_does_not_expose_capture_from_another_workspace_or_listener() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let application = application_with_workspace_ports(Arc::clone(&ports), Arc::clone(&workspaces));
    let workspace_id = workspaces.list().await.expect("workspace summaries")[0].id;
    let listener_id = workspaces
        .get(workspace_id)
        .await
        .expect("workspace")
        .listeners[0]
        .id;

    for (capture_workspace_id, capture_listener_id) in [
        (WorkspaceId::new(), listener_id),
        (workspace_id, ListenerId::new()),
    ] {
        let capture_id = SocketCaptureId::new();
        *ports.socket_capture_detail.lock() = Some(capture_detail(
            capture_id,
            capture_workspace_id,
            capture_listener_id,
        ));

        let report = application
            .diagnostic_report_generate(DiagnosticReportQuery {
                workspace_id,
                listener_id,
                capture_id: Some(capture_id),
            })
            .await
            .expect("scope mismatch remains a partial report");

        assert!(report.bundle.capture_detail.is_none());
        assert!(report.bundle.collection_errors.iter().any(|error| {
            error.section == DiagnosticReportSection::CaptureDetail
                && error.code == "CAPTURE_SCOPE_MISMATCH"
        }));
        assert!(!report.markdown.contains("指定 Capture 测试数据"));
    }

    let capture_id = SocketCaptureId::new();
    *ports.socket_capture_detail.lock() =
        Some(capture_detail(capture_id, workspace_id, listener_id));
    let report = application
        .diagnostic_report_generate(DiagnosticReportQuery {
            workspace_id,
            listener_id,
            capture_id: Some(capture_id),
        })
        .await
        .expect("same-scope capture is available");
    assert_eq!(
        report
            .bundle
            .capture_detail
            .as_ref()
            .map(|detail| detail.record.capture_id),
        Some(capture_id)
    );
    assert!(report.markdown.contains("指定 Capture 测试数据"));
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
            capture_id: None,
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
            capture_id: None,
        })
        .await
        .expect("partial report remains available");

    assert!(report.bundle.protocol_package_detail.is_none());
    assert!(report.bundle.collection_errors.iter().any(|error| {
        error.section == DiagnosticReportSection::ProtocolPackageDetail
            && error.code == "PROTOCOL_PACKAGE_NOT_FOUND"
    }));
    assert!(report.markdown.contains("missing-external@1.2.3"));
    let queries = ports.socket_capture_queries.lock();
    assert_eq!(queries[0].package.as_ref(), Some(&package));
}
