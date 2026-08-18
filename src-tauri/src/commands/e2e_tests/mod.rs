//! T30 跨命令、持久化、脚本与真实 TCP 的发布前集成证据。

mod support;

use std::{
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    time::{Duration, Instant},
};

use intercept_proxy_application::{
    DocumentAction, DocumentFieldName, DocumentValue, PageRequest, ProtocolPackageDetailViewModel,
    ProtocolPackageImportPreviewViewModel, ProtocolPackageImportViewModel,
    ProtocolPackageVersionViewModel, ProxyListener, ProxyWorkspace, SocketCaptureDetailViewModel,
    SocketCaptureDocumentValue, SocketCaptureKind, SocketCapturePageViewModel,
    SocketCapturePayload, SocketCaptureQuery, SocketCaptureSort, SocketDirection,
    SocketDisplayResult, SocketDocumentRuleDefinition, SocketRuleSaveInput, SocketWriteKind,
    SortDirection, WorkspaceSummaryViewModel,
};
use intercept_proxy_domain::{
    DirectionProcessingOptions, ListenerDataPlane, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, ScriptedSocketProcessing, SocketDownstreamSecurity,
    SocketLocalResponderTopology, SocketPayloadProcessing, SocketRelaySettings, SocketTopology,
};
use serde_json::json;

use self::support::{CrossLayerFixture, unused_local_port};

const REQUEST: &[u8; 18] = b"020012345600001000";
const RESPONSE: &[u8; 20] = b"02101234560000100000";

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
    assert_eq!(preview.schema.id, "t30-iso8583");
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
    assert_eq!(detail.schema.id, "t30-iso8583");
    assert_eq!(detail.schema.fields.len(), 4);
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
        processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: package.clone(),
            upstream: DirectionProcessingOptions {
                decode_enabled: true,
                encode_enabled: false,
            },
            downstream: DirectionProcessingOptions {
                decode_enabled: false,
                encode_enabled: true,
            },
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

    let rule: SocketDocumentRuleDefinition = fixture.invoke_ok(
        &webview,
        "socket_rule_save",
        json!({
            "input": SocketRuleSaveInput {
                rule_id: None,
                expected_revision: None,
                name: "本机应答".into(),
                enabled: true,
                priority: 10,
                listener_id: listener.id,
                package: package.clone(),
                schema_version: 1,
                direction: SocketDirection::Downstream,
                conditions: Vec::new(),
                actions: vec![
                    DocumentAction::SetField {
                        field: DocumentFieldName::new("mti").unwrap(),
                        value: DocumentValue::String("0210".into()),
                    },
                    DocumentAction::SetField {
                        field: DocumentFieldName::new("response_code").unwrap(),
                        value: DocumentValue::String("00".into()),
                    },
                ],
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

    assert_eq!(
        tcp_exchange(listener.port, REQUEST, RESPONSE.len()),
        RESPONSE
    );
    let capture = wait_for_capture(&fixture, &webview, selected.id, listener.id);
    let SocketCapturePayload::LocalExchange(exchange) = capture.record.payload else {
        panic!("expected a formal LocalExchange capture")
    };
    assert_eq!(exchange.request_origin, REQUEST);
    assert_eq!(exchange.written_response, RESPONSE);
    assert_eq!(exchange.response_write_kind, SocketWriteKind::Encoded);
    assert_eq!(
        exchange.response_display,
        SocketDisplayResult::UntrustedHtml {
            html: "<p>T30 ISO response</p>".into(),
        }
    );
    assert_eq!(exchange.matched_downstream_rule_ids, [rule.rule_id()]);
    let request_document = exchange.request_document.expect("request was decoded");
    assert_eq!(
        request_document.get("mti"),
        Some(&SocketCaptureDocumentValue::String("0200".into()))
    );
    assert!(request_document.get("response_code").is_none());
    assert_eq!(
        exchange.response_document.get("mti"),
        Some(&SocketCaptureDocumentValue::String("0210".into()))
    );
    assert_eq!(
        exchange.response_document.get("response_code"),
        Some(&SocketCaptureDocumentValue::String("00".into()))
    );

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
    stream.read_exact(&mut response).unwrap();
    stream.shutdown(Shutdown::Both).unwrap();
    response
}

fn wait_for_capture(
    fixture: &CrossLayerFixture,
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    workspace_id: intercept_proxy_application::WorkspaceId,
    listener_id: intercept_proxy_application::ListenerId,
) -> SocketCaptureDetailViewModel {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let page: SocketCapturePageViewModel = fixture.invoke_ok(
            webview,
            "socket_capture_query",
            json!({
                "query": SocketCaptureQuery {
                    workspace_id: Some(workspace_id),
                    listener_id: Some(listener_id),
                    session_id: None,
                    connection_id: None,
                    package: Some(package_ref()),
                    direction: None,
                    kind: Some(SocketCaptureKind::LocalExchange),
                    occurred_from: None,
                    occurred_to: None,
                    sort: SocketCaptureSort::OccurredAt,
                    direction_sort: SortDirection::Asc,
                    page: PageRequest { page: 1, page_size: 10 },
                }
            }),
        );
        if let Some(row) = page.rows.first() {
            return fixture.invoke_ok(
                webview,
                "socket_capture_get_detail",
                json!({ "captureId": row.capture_id }),
            );
        }
        assert!(
            Instant::now() < deadline,
            "formal capture was not persisted"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
