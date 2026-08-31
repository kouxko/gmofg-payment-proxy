// `rule_parse_document_value` 的真实 Tauri IPC 参数与错误映射测试。

use intercept_proxy_application::{AppErrorViewModel, DocumentValue as TestDocumentValue};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tauri::{
    WebviewUrl, WebviewWindowBuilder, http::HeaderMap, ipc::InvokeBody, test::MockRuntime,
};

#[test]
fn editor_context_command_is_registered_and_exported_with_camel_case_arguments() {
    let command_registry = include_str!("../mod.rs");
    assert!(command_registry.contains("rule_editor_context,"));

    let bindings = include_str!("../../../../src/generated/rust-types.ts");
    assert!(bindings.contains("ruleEditorContext: (listenerId: ListenerId)"));
    assert!(bindings.contains("__TAURI_INVOKE(\"rule_editor_context\", { listenerId })"));
    assert!(bindings.contains("export type RuleEditorContext = {"));
    assert!(bindings.contains("new_rule_draft: RuleDefinitionSaveInput"));
    assert!(bindings.contains("local_document_types: RuleLocalDocumentTypeCapability[]"));
    assert!(bindings.contains("ruleDefinitionDocumentConditionDraft:"));
    assert!(bindings.contains("ruleDefinitionDocumentActionDraft:"));
    assert!(bindings.contains("export type RuleLocalDocumentValueType = \"string\" | \"number\" | \"boolean\" | \"null\" | \"object\" | \"array\";"));
    assert!(bindings.contains("event: \"processed\""));
    assert!(bindings.contains("event: \"encoded\""));
}

#[test]
fn ipc_returns_all_recursive_document_value_variants() {
    let app = test_app();
    let webview = test_webview(&app);
    for (field_type, raw, expected) in [
        ("string", "金额", TestDocumentValue::String("金额".into())),
        (
            "number",
            "-42",
            intercept_proxy_domain::Document::parse_json("-42")
                .unwrap()
                .root()
                .clone(),
        ),
        ("boolean", "false", TestDocumentValue::Boolean(false)),
        (
            "object",
            r#"{"nested":null}"#,
            intercept_proxy_domain::Document::parse_json(r#"{"nested":null}"#)
                .unwrap()
                .root()
                .clone(),
        ),
        (
            "array",
            r#"[1,"two",false]"#,
            intercept_proxy_domain::Document::parse_json(r#"[1,"two",false]"#)
                .unwrap()
                .root()
                .clone(),
        ),
    ] {
        let actual: TestDocumentValue =
            invoke_ok(&webview, json!({ "fieldType": field_type, "raw": raw }));
        assert_eq!(actual, expected);
    }
}

#[test]
fn ipc_preserves_exact_boundaries_and_returns_payload_free_errors() {
    let app = test_app();
    let webview = test_webview(&app);
    let exact_string = "x".repeat(16 * 1_024);
    let actual: TestDocumentValue = invoke_ok(
        &webview,
        json!({ "fieldType": "string", "raw": exact_string }),
    );
    assert_eq!(actual, TestDocumentValue::String("x".repeat(16 * 1_024)));

    let secret = "merchant-secret-not-json";
    let malformed = invoke_error(&webview, json!({ "fieldType": "object", "raw": secret }));
    assert_eq!(malformed.code, "JSON_INVALID");
    assert!(!format!("{malformed:?}").contains(secret));

    let oversized = invoke_error(
        &webview,
        json!({ "fieldType": "string", "raw": "x".repeat(16 * 1_024 + 1) }),
    );
    assert_eq!(oversized.code, "PROTOCOL_RULE_VALUE_TOO_LARGE");
    assert!(oversized.field_errors.contains_key("raw"));

    let wrong_object = invoke_error(&webview, json!({ "fieldType": "object", "raw": "[]" }));
    assert_eq!(wrong_object.code, "PROTOCOL_RULE_VALUE_INVALID");
    assert!(wrong_object.field_errors.contains_key("raw"));
}

fn test_app() -> tauri::App<MockRuntime> {
    tauri::test::mock_builder()
        .invoke_handler(tauri::generate_handler![rule_parse_document_value])
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
        cmd: "rule_parse_document_value".into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: InvokeBody::Json(body),
        headers: HeaderMap::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_owned(),
    }
}
