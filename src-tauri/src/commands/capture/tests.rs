//! Socket capture 命令的真实 Tauri IPC 边界测试。

use std::{path::PathBuf, sync::Arc};

use intercept_proxy_application::{
    AppErrorViewModel, AppResult, OperationResultViewModel, PageRequest, RuntimeEpoch,
    SocketCaptureDetailViewModel, SocketCaptureDocument, SocketCaptureId,
    SocketCapturePageViewModel, SocketCapturePayload, SocketCaptureQuery, SocketCaptureRecord,
    SocketCaptureSchemaRef, SocketCaptureSort, SocketDisplayFallbackReason, SocketDisplayResult,
    SocketRelayFrameCapture, SocketWriteKind, SortDirection,
};
use intercept_proxy_domain::{
    Document, DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema,
    DocumentSchemaId, DocumentValue, ListenerId, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, SocketDirection, WorkspaceId,
};
use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use intercept_proxy_infrastructure::{
    InfrastructureError, NativeFileDialog, SecretProtector, SocketCaptureRepositoryAdapter,
    SqliteStore, adapters::FileSelection,
};
use intercept_proxy_product_api::InterceptProxyProfile;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tauri::{
    Manager, WebviewUrl, WebviewWindowBuilder, http::HeaderMap, ipc::InvokeBody, test::MockRuntime,
};
use tempfile::TempDir;

use super::{socket_capture_clear, socket_capture_get_detail, socket_capture_query};
use crate::app_state::AppState;

#[test]
fn query_returns_a_typed_socket_capture_page_through_real_tauri_ipc() {
    let fixture = TempDir::new().unwrap();
    let expected = record();
    let app = test_app(fixture.path(), Some(&expected));
    let webview = test_webview(&app);

    let page: SocketCapturePageViewModel = invoke_ok(
        &webview,
        "socket_capture_query",
        json!({ "query": query() }),
    );

    assert_eq!(page.total, 1);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].capture_id, expected.capture_id);
}

#[test]
fn detail_returns_the_typed_socket_record_through_real_tauri_ipc() {
    let fixture = TempDir::new().unwrap();
    let expected = record();
    let app = test_app(fixture.path(), Some(&expected));
    let webview = test_webview(&app);

    let detail: SocketCaptureDetailViewModel = invoke_ok(
        &webview,
        "socket_capture_get_detail",
        json!({ "captureId": expected.capture_id }),
    );

    assert_eq!(detail.record.capture_id, expected.capture_id);
    assert_eq!(detail.record.payload, expected.payload);
    assert_eq!(detail.record.completed_at, expected.completed_at);
}

#[test]
fn detail_preserves_integers_beyond_javascript_safe_range_through_real_tauri_ipc() {
    let fixture = TempDir::new().unwrap();
    let expected = record_with_lossless_integers();
    let app = test_app(fixture.path(), Some(&expected));
    let webview = test_webview(&app);

    let detail: Value = invoke_ok(
        &webview,
        "socket_capture_get_detail",
        json!({ "captureId": expected.capture_id }),
    );

    assert_eq!(
        detail.pointer("/record/payload/capture/document/values/0/value"),
        Some(&json!("9007199254740993"))
    );
    assert_eq!(
        detail.pointer("/record/payload/capture/document/values/1/value"),
        Some(&json!("-9007199254740993"))
    );
}

#[test]
fn serialized_socket_detail_contains_no_http_only_fields() {
    let fixture = TempDir::new().unwrap();
    let expected = record();
    let app = test_app(fixture.path(), Some(&expected));
    let webview = test_webview(&app);

    let detail: Value = invoke_ok(
        &webview,
        "socket_capture_get_detail",
        json!({ "captureId": expected.capture_id }),
    );

    assert_no_http_only_fields(&detail);
}

#[test]
fn detail_maps_missing_capture_to_a_structured_not_found_error() {
    let fixture = TempDir::new().unwrap();
    let missing = SocketCaptureId::new();
    let app = test_app(fixture.path(), None);
    let webview = test_webview(&app);

    let error = invoke_error(
        &webview,
        "socket_capture_get_detail",
        json!({ "captureId": missing }),
    );

    assert_eq!(error.code, "SOCKET_CAPTURE_NOT_FOUND");
    let missing_id = missing.to_string();
    assert_eq!(error.entity_id.as_deref(), Some(missing_id.as_str()));
}

#[test]
fn clear_rejects_an_unconfirmed_request_through_real_tauri_ipc() {
    let fixture = TempDir::new().unwrap();
    let app = test_app(fixture.path(), None);
    let webview = test_webview(&app);

    let error = invoke_error(
        &webview,
        "socket_capture_clear",
        json!({ "workspaceId": selected_workspace_id(&app), "confirmed": false }),
    );

    assert_eq!(error.code, "CONFIRMATION_REQUIRED");
}

#[test]
fn clear_confirmed_returns_success_and_removes_completed_socket_captures() {
    let fixture = TempDir::new().unwrap();
    let expected = record();
    let app = test_app(fixture.path(), Some(&expected));
    let webview = test_webview(&app);
    let workspace_id = selected_workspace_id(&app);

    let result: OperationResultViewModel = invoke_ok(
        &webview,
        "socket_capture_clear",
        json!({ "workspaceId": workspace_id, "confirmed": true }),
    );
    let page: SocketCapturePageViewModel = invoke_ok(
        &webview,
        "socket_capture_query",
        json!({ "query": query() }),
    );

    assert!(result.success);
    assert!(!result.cancelled);
    assert_eq!(page.total, 0);
}

#[test]
fn clear_rejects_a_workspace_switched_after_confirmation_without_deleting_records() {
    let fixture = TempDir::new().unwrap();
    let expected = record();
    let app = test_app(fixture.path(), Some(&expected));
    let originally_selected = selected_workspace_id(&app);
    let state = app.state::<AppState>();
    let replacement =
        tauri::async_runtime::block_on(state.application.workspace_create("Replacement".into()))
            .unwrap();
    tauri::async_runtime::block_on(state.application.workspace_select(replacement.id)).unwrap();
    let webview = test_webview(&app);

    let error = invoke_error(
        &webview,
        "socket_capture_clear",
        json!({ "workspaceId": originally_selected, "confirmed": true }),
    );
    let page: SocketCapturePageViewModel = invoke_ok(
        &webview,
        "socket_capture_query",
        json!({ "query": query() }),
    );

    assert_eq!(error.code, "WORKSPACE_SELECTION_CHANGED");
    assert_eq!(error.entity_id, Some(originally_selected.to_string()));
    assert_eq!(page.total, 1, "拒绝错误目标后不得删除任何记录");
}

#[derive(Debug)]
struct NoFileDialog;

impl NativeFileDialog for NoFileDialog {
    fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
        Ok(None)
    }

    fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct TestSecretProtector;

impl SecretProtector for TestSecretProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(plaintext.to_vec())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(ciphertext.to_vec())
    }
}

fn test_app(
    data_dir: &std::path::Path,
    seeded_record: Option<&SocketCaptureRecord>,
) -> tauri::App<MockRuntime> {
    let host = tauri::async_runtime::block_on(
        ApplicationHostBuilder::new(
            data_dir,
            HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
            Arc::new(InterceptProxyProfile),
        )
        .build(),
    )
    .unwrap();
    if let Some(record) = seeded_record {
        let selected = tauri::async_runtime::block_on(host.application().workspace_list())
            .unwrap()
            .into_iter()
            .find(|workspace| workspace.selected)
            .unwrap();
        let mut record = record.clone();
        record.workspace_id = selected.id;
        let store = Arc::new(SqliteStore::open(&data_dir.join("intercept-proxy.sqlite3")).unwrap());
        SocketCaptureRepositoryAdapter::new(store)
            .record(record)
            .unwrap();
    }
    tauri::test::mock_builder()
        .manage(AppState::new(host))
        .invoke_handler(tauri::generate_handler![
            socket_capture_query,
            socket_capture_get_detail,
            socket_capture_clear,
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap()
}

fn test_webview(app: &tauri::App<MockRuntime>) -> tauri::WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .build()
        .unwrap()
}

fn selected_workspace_id(app: &tauri::App<MockRuntime>) -> WorkspaceId {
    let state = app.state::<AppState>();
    tauri::async_runtime::block_on(state.application.workspace_list())
        .unwrap()
        .into_iter()
        .find(|workspace| workspace.selected)
        .expect("test app has one selected workspace")
        .id
}

fn invoke_ok<T: DeserializeOwned>(
    webview: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> T {
    tauri::test::get_ipc_response(webview, request(command, body))
        .unwrap()
        .deserialize()
        .unwrap()
}

fn invoke_error(
    webview: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> AppErrorViewModel {
    serde_json::from_value(
        tauri::test::get_ipc_response(webview, request(command, body)).unwrap_err(),
    )
    .unwrap()
}

fn request(command: &str, body: Value) -> tauri::webview::InvokeRequest {
    tauri::webview::InvokeRequest {
        cmd: command.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: if cfg!(any(windows, target_os = "android")) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(body),
        headers: HeaderMap::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_owned(),
    }
}

fn query() -> SocketCaptureQuery {
    SocketCaptureQuery {
        workspace_id: None,
        listener_id: None,
        session_id: None,
        connection_id: None,
        package: None,
        direction: None,
        kind: None,
        occurred_from: None,
        occurred_to: None,
        sort: SocketCaptureSort::OccurredAt,
        direction_sort: SortDirection::Desc,
        page: PageRequest {
            page: 1,
            page_size: 20,
        },
    }
}

fn record() -> SocketCaptureRecord {
    let occurred_at = "2026-08-15T10:00:00Z".parse().unwrap();
    let connection_id = intercept_proxy_application::SocketConnectionId::new();
    SocketCaptureRecord {
        capture_id: SocketCaptureId::new(),
        runtime_epoch: RuntimeEpoch::new_v4(),
        workspace_id: WorkspaceId::new(),
        listener_id: ListenerId::new(),
        session_id: connection_id.as_uuid(),
        connection_id,
        peer_address: "127.0.0.1:43100".to_owned(),
        occurred_at,
        completed_at: "2026-08-15T10:00:00.005Z".parse().unwrap(),
        payload: SocketCapturePayload::RelayFrame(SocketRelayFrameCapture {
            direction: SocketDirection::Upstream,
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("iso8583").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            schema: SocketCaptureSchemaRef {
                id: DocumentSchemaId::new("payment").unwrap(),
                version: 1,
            },
            decode_enabled: false,
            encode_enabled: false,
            origin: vec![0x02, 0x10, 0x03],
            document: None,
            matched_rule_ids: Vec::new(),
            written: vec![0x02, 0x10, 0x03],
            write_kind: SocketWriteKind::Original,
            display: SocketDisplayResult::HexFallback {
                reason: SocketDisplayFallbackReason::EncodeDisabled,
                diagnostic: None,
            },
        }),
    }
}

fn record_with_lossless_integers() -> SocketCaptureRecord {
    let schema = DocumentSchema::new(
        DocumentSchemaId::new("integer-boundary").unwrap(),
        1,
        "Integer boundary",
        vec![
            DocumentField::new(
                DocumentFieldName::new("positive").unwrap(),
                DocumentFieldType::Int,
                "Positive",
            )
            .unwrap(),
            DocumentField::new(
                DocumentFieldName::new("negative").unwrap(),
                DocumentFieldType::Int,
                "Negative",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let mut document = Document::new(schema.clone());
    document
        .set("positive", DocumentValue::Int(9_007_199_254_740_993))
        .unwrap();
    document
        .set("negative", DocumentValue::Int(-9_007_199_254_740_993))
        .unwrap();

    let mut record = record();
    let SocketCapturePayload::RelayFrame(frame) = &mut record.payload else {
        unreachable!();
    };
    frame.schema = SocketCaptureSchemaRef {
        id: schema.id().clone(),
        version: schema.version(),
    };
    frame.decode_enabled = true;
    frame.document = Some(SocketCaptureDocument::from_document(&document));
    record
}

fn assert_no_http_only_fields(value: &Value) {
    const HTTP_ONLY_FIELDS: &[&str] = &[
        "method",
        "target",
        "http_status",
        "start_line_bytes",
        "raw_headers",
        "headers",
        "body_text",
        "body_bytes",
        "json",
        "query_string",
        "json_path",
    ];
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                assert!(
                    !HTTP_ONLY_FIELDS.contains(&key.as_str()),
                    "HTTP-only field leaked into Socket DTO: {key}"
                );
                assert_no_http_only_fields(nested);
            }
        }
        Value::Array(items) => items.iter().for_each(assert_no_http_only_fields),
        _ => {}
    }
}
