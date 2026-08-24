//! T30 跨命令、持久化、脚本与真实 TCP 的发布前集成证据。

mod support;

use std::{
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    time::{Duration, Instant},
};

use intercept_proxy_application::{
    DiagnosticLogPageViewModel, DiagnosticReportQuery, DocumentAction, DocumentFieldName,
    DocumentValue, ProtocolDocumentRuleDefinition, ProtocolPackageDetailViewModel,
    ProtocolPackageImportPreviewViewModel, ProtocolPackageImportViewModel,
    ProtocolPackageVersionViewModel, ProtocolRuleSaveInput, ProtocolRuleStage, ProxyListener,
    ProxyWorkspace, WorkspaceSummaryViewModel,
};
use intercept_proxy_domain::{
    ListenerDataPlane, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    ScriptedSocketProcessing, SocketDownstreamSecurity, SocketLocalResponderTopology,
    SocketPayloadProcessing, SocketRelaySettings, SocketTopology,
};
use serde_json::json;

use self::support::{CrossLayerFixture, unused_local_port};
use super::DiagnosticReportExportOutcome;

const REQUEST: &[u8; 18] = b"020012345600001000";
const RESPONSE: &[u8; 20] = b"02101234560000100000";

#[test]
fn diagnostic_report_export_uses_native_dialog_and_writes_copyable_markdown() {
    let fixture = CrossLayerFixture::new();
    let webview = fixture.webview();
    let selected = selected_workspace(&fixture, &webview);
    let workspace: ProxyWorkspace = fixture.invoke_ok(
        &webview,
        "workspace_get",
        json!({ "workspaceId": selected.id }),
    );
    let listener = workspace.listeners.first().expect("default listener");

    let outcome: Option<DiagnosticReportExportOutcome> = fixture.invoke_ok(
        &webview,
        "diagnostic_reproduction_report_export",
        json!({
            "query": DiagnosticReportQuery {
                workspace_id: workspace.id,
                listener_id: listener.id,
            }
        }),
    );

    assert!(outcome.expect("saved report").bytes_written > 0);
    let markdown = fixture.saved_report();
    assert!(markdown.contains("# Intercept Proxy 故障复现报告"));
    assert!(markdown.contains(&workspace.id.to_string()));
    assert!(markdown.contains(&listener.id.to_string()));
    assert!(markdown.contains("## 持久化应用运行日志"));
    fixture.assert_report_dialog_boundary();
}

#[test]
fn mcp_reproduction_report_and_runtime_logs_are_read_only_and_queryable() {
    let fixture = CrossLayerFixture::new();
    let webview = fixture.webview();
    let selected = selected_workspace(&fixture, &webview);
    let before: ProxyWorkspace = fixture.invoke_ok(
        &webview,
        "workspace_get",
        json!({ "workspaceId": selected.id }),
    );
    let listener = before.listeners.first().expect("default listener");

    let logs = fixture.call_mcp_tool("application_log_query", json!({ "limit": 10 }));
    let report = fixture.call_mcp_tool(
        "reproduction_report",
        json!({
            "workspace_id": before.id,
            "listener_id": listener.id,
        }),
    );
    let after: ProxyWorkspace = fixture.invoke_ok(
        &webview,
        "workspace_get",
        json!({ "workspaceId": selected.id }),
    );

    assert!(logs["rows"].is_array());
    assert!(
        report["markdown"]
            .as_str()
            .is_some_and(|markdown| markdown.contains("持久化应用运行日志"))
    );
    assert_eq!(before, after, "read-only MCP must not mutate the workspace");
}

#[test]
// 一条测试刻意保留单一 host/SQLite/runtime 所有权，拆成多个测试会把跨层证据降级为孤立断言。
#[allow(clippy::too_many_lines)]
fn iso_local_responder_crosses_real_ipc_sqlite_rhai_tcp_and_capture() {
    let fixture = CrossLayerFixture::new();
    let webview = fixture.webview();
    let package = package_ref();

    let preview: ProtocolPackageImportPreviewViewModel =
        fixture.invoke_ok(&webview, "protocol_package_import", json!({}));
    assert_eq!(preview.package, package);
    assert_eq!(preview.upstream_schema.id, "t30-iso8583");
    assert_eq!(preview.downstream_schema.id, "t30-iso8583");
    let imported: ProtocolPackageImportViewModel = fixture.invoke_ok(
        &webview,
        "protocol_package_import_commit",
        json!({ "token": preview.token }),
    );
    assert_eq!(imported.version.package, package);

    let detail: ProtocolPackageDetailViewModel = fixture.invoke_ok(
        &webview,
        "protocol_package_detail",
        json!({ "packageRef": package }),
    );
    assert_eq!(detail.upstream_schema.id, "t30-iso8583");
    assert_eq!(detail.downstream_schema.id, "t30-iso8583");
    assert_eq!(detail.upstream_schema.fields.len(), 1);
    let enabled: ProtocolPackageVersionViewModel = fixture.invoke_ok(
        &webview,
        "protocol_package_enable",
        json!({ "packageRef": package }),
    );
    assert!(enabled.enabled);

    let selected = selected_workspace(&fixture, &webview);
    let mut listener: ProxyListener = fixture.invoke_ok(&webview, "listener_new", json!({}));
    listener.name = "T30 ISO LocalResponder".into();
    listener.enabled = true;
    listener.port = unused_local_port();
    listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology: SocketTopology::LocalResponder(SocketLocalResponderTopology {
            downstream_security: SocketDownstreamSecurity::Tcp,
        }),
        maximum_connections: 4,
        runtime_limits: intercept_proxy_domain::SocketRuntimeLimits::default(),
        processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: package.clone(),
        }),
    });
    let saved: ProxyWorkspace = fixture.invoke_ok(
        &webview,
        "listener_save",
        json!({
            "workspaceId": selected.id,
            "expectedWorkspaceRevision": selected.revision,
            "listener": listener,
            "certificateReferences": [],
        }),
    );
    assert!(saved.listeners.contains(&listener));

    let _rule: ProtocolDocumentRuleDefinition = fixture.invoke_ok(
        &webview,
        "protocol_rule_save",
        json!({
            "input": ProtocolRuleSaveInput {
                rule_id: None,
                expected_revision: None,
                name: "本机应答".into(),
                enabled: true,
                priority: 10,
                listener_id: listener.id,
                package: package.clone(),
                schema_version: 1,
                stage: ProtocolRuleStage::ProxyToApp,
                conditions: Vec::new(),
                actions: vec![DocumentAction::SetField {
                    field: DocumentFieldName::new("message").unwrap(),
                    value: DocumentValue::Blob(RESPONSE.to_vec()),
                }],
            }
        }),
    );
    let configured: ProxyWorkspace = fixture.invoke_ok(
        &webview,
        "workspace_get",
        json!({ "workspaceId": selected.id }),
    );
    let _: serde_json::Value = fixture.invoke_ok(
        &webview,
        "listener_start",
        json!({
            "workspaceId": selected.id,
            "expectedWorkspaceRevision": configured.revision,
            "listenerId": listener.id,
        }),
    );

    let response = tcp_exchange(listener.port, REQUEST, RESPONSE.len());
    if response != RESPONSE {
        let diagnostics: DiagnosticLogPageViewModel = fixture.invoke_ok(
            &webview,
            "diagnostic_log_query",
            json!({ "query": { "keyword": null, "after_event_id": null, "limit": 300 } }),
        );
        panic!(
            "unexpected response {response:?}; diagnostics={:?}",
            diagnostics.rows
        );
    }

    let _: serde_json::Value = fixture.invoke_ok(
        &webview,
        "listener_stop",
        json!({
            "workspaceId": selected.id,
            "expectedWorkspaceRevision": configured.revision,
            "listenerId": listener.id,
        }),
    );
    fixture.assert_dialog_boundaries();
}

fn package_ref() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("t30-iso-local").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn selected_workspace(
    fixture: &CrossLayerFixture,
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
) -> WorkspaceSummaryViewModel {
    fixture
        .invoke_ok::<Vec<WorkspaceSummaryViewModel>>(webview, "workspace_list", json!({}))
        .into_iter()
        .find(|workspace| workspace.selected)
        .expect("default selected workspace")
}

fn tcp_exchange(port: u16, request: &[u8], response_len: usize) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("listener did not accept TCP connections: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(request).unwrap();
    let mut response = vec![0; response_len];
    let mut received = 0;
    while received < response_len {
        match stream.read(&mut response[received..]).unwrap() {
            0 => break,
            count => received += count,
        }
    }
    response.truncate(received);
    let _ = stream.shutdown(Shutdown::Both);
    response
}
