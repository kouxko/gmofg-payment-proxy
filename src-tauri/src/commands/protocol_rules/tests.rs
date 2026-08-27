//! `protocol_rule_parse_value` 的真实 Tauri IPC 参数与错误映射测试。

use intercept_proxy_application::{AppErrorViewModel, DocumentValue};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tauri::{
    WebviewUrl, WebviewWindowBuilder, http::HeaderMap, ipc::InvokeBody, test::MockRuntime,
};

use super::protocol_rule_parse_value;

#[test]
fn editor_context_command_is_registered_and_exported_with_camel_case_arguments() {
    let command_registry = include_str!("../mod.rs");
    assert!(command_registry.contains("protocol_rule_editor_context,"));

    let bindings = include_str!("../../../../src/generated/rust-types.ts");
    assert!(bindings.contains("protocolRuleEditorContext: (listenerId: ListenerId)"));
    assert!(bindings.contains("__TAURI_INVOKE(\"protocol_rule_editor_context\", { listenerId })"));
    assert!(bindings.contains("export type ProtocolRuleEditorContext = {"));
    assert!(bindings.contains("new_rule_draft: ProtocolRuleSaveInput"));
}

#[test]
fn ipc_returns_all_four_strict_document_value_variants() {
    let app = test_app();
    let webview = test_webview(&app);
    for (field_type, raw, expected) in [
        ("string", "金额", DocumentValue::String("金额".into())),
        ("int", "-42", DocumentValue::Int(-42)),
        ("bool", "false", DocumentValue::Bool(false)),
        (
            "blob",
            "01:a0-FF",
            DocumentValue::Blob(vec![0x01, 0xA0, 0xFF]),
        ),
    ] {
        let actual: DocumentValue =
            invoke_ok(&webview, json!({ "fieldType": field_type, "raw": raw }));
        assert_eq!(actual, expected);
    }
}

#[test]
fn ipc_preserves_exact_boundaries_and_returns_payload_free_errors() {
    let app = test_app();
    let webview = test_webview(&app);
    let exact_string = "x".repeat(16 * 1_024);
    let actual: DocumentValue = invoke_ok(
        &webview,
        json!({ "fieldType": "string", "raw": exact_string }),
    );
    assert_eq!(actual, DocumentValue::String("x".repeat(16 * 1_024)));

    let exact_blob = "AA".repeat(64 * 1_024);
    let DocumentValue::Blob(actual) =
        invoke_ok::<DocumentValue>(&webview, json!({ "fieldType": "blob", "raw": exact_blob }))
    else {
        panic!("Blob IPC result must keep its tagged type")
    };
    assert_eq!(actual.len(), 64 * 1_024);

    let secret = "merchant-secret-not-hex";
    let malformed = invoke_error(&webview, json!({ "fieldType": "blob", "raw": secret }));
    assert_eq!(malformed.code, "PROTOCOL_RULE_VALUE_INVALID");
    assert!(malformed.field_errors.contains_key("raw"));
    assert!(!format!("{malformed:?}").contains(secret));

    let oversized = invoke_error(
        &webview,
        json!({ "fieldType": "string", "raw": "x".repeat(16 * 1_024 + 1) }),
    );
    assert_eq!(oversized.code, "PROTOCOL_RULE_VALUE_TOO_LARGE");
    assert!(oversized.field_errors.contains_key("raw"));

    let exact_int = format!("{}1", " ".repeat(127));
    assert_eq!(
        invoke_ok::<DocumentValue>(&webview, json!({ "fieldType": "int", "raw": exact_int }),),
        DocumentValue::Int(1)
    );
    let exact_long_digits = invoke_error(
        &webview,
        json!({ "fieldType": "int", "raw": "1".repeat(128) }),
    );
    assert_eq!(exact_long_digits.code, "PROTOCOL_RULE_VALUE_INVALID");
    for raw in [format!("{}1", " ".repeat(128)), "1".repeat(129)] {
        let oversized_int = invoke_error(&webview, json!({ "fieldType": "int", "raw": raw }));
        assert_eq!(oversized_int.code, "PROTOCOL_RULE_VALUE_TOO_LARGE");
        assert!(oversized_int.field_errors.contains_key("raw"));
    }
}

fn test_app() -> tauri::App<MockRuntime> {
    tauri::test::mock_builder()
        .invoke_handler(tauri::generate_handler![protocol_rule_parse_value])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap()
}

fn test_webview(app: &tauri::App<MockRuntime>) -> tauri::WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .build()
        .unwrap()
}

fn invoke_ok<T: DeserializeOwned>(webview: &tauri::WebviewWindow<MockRuntime>, body: Value) -> T {
    tauri::test::get_ipc_response(webview, request(body))
        .unwrap()
        .deserialize()
        .unwrap()
}

fn invoke_error(webview: &tauri::WebviewWindow<MockRuntime>, body: Value) -> AppErrorViewModel {
    serde_json::from_value(tauri::test::get_ipc_response(webview, request(body)).unwrap_err())
        .unwrap()
}

fn request(body: Value) -> tauri::webview::InvokeRequest {
    tauri::webview::InvokeRequest {
        cmd: "protocol_rule_parse_value".into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: InvokeBody::Json(body),
        headers: HeaderMap::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_owned(),
    }
}
