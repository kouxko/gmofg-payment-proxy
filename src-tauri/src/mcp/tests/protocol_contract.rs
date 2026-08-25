use serde_json::{Value, json};

use super::{backend, post, protocol, request_meta, start_test_server};

#[tokio::test]
async fn closed_nested_input_schema_is_enforced_before_backend_execution() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let response = post(
        server.local_addr(),
        "tools/call",
        Some("exchange_observation_query"),
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "_meta": request_meta(),
                "name": "exchange_observation_query",
                "arguments": {
                    "workspace_id": "00000000-0000-0000-0000-000000000000",
                    "page": {"page": 1, "page_size": 50, "unexpected": true}
                }
            }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "INVALID_ARGUMENTS"
    );
    assert!(
        response["result"]["structuredContent"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("page.unexpected"))
    );
    server.shutdown().await;
}

#[tokio::test]
async fn successful_output_root_must_match_the_advertised_schema() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let response = post(
        server.local_addr(),
        "tools/call",
        Some("settings_get"),
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "_meta": request_meta(),
                "name": "settings_get",
                "arguments": {}
            }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "OUTPUT_SCHEMA_MISMATCH"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn published_value_constraints_are_enforced_before_backend_execution() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let response = post(
        server.local_addr(),
        "tools/call",
        Some("diagnostics_query"),
        json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "_meta": request_meta(),
                "name": "diagnostics_query",
                "arguments": {"limit": 501}
            }
        }),
    )
    .await;

    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "INVALID_ARGUMENTS"
    );
    assert!(
        response["result"]["structuredContent"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("limit"))
    );
    server.shutdown().await;
}

#[tokio::test]
async fn array_and_nullable_successful_output_roots_are_accepted() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    for (id, name, expected) in [
        (6, "workspace_list", json!([])),
        (7, "android_runtime_owner", Value::Null),
    ] {
        let response = post(
            server.local_addr(),
            "tools/call",
            Some(name),
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {"_meta": request_meta(), "name": name, "arguments": {}}
            }),
        )
        .await;
        assert_eq!(response["result"]["isError"], false, "{response}");
        assert_eq!(response["result"]["structuredContent"], expected);
    }
    server.shutdown().await;
}

#[tokio::test]
async fn logical_input_and_output_budgets_are_enforced() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let oversized_input = post(
        server.local_addr(),
        "tools/call",
        Some("workspace_get"),
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "_meta": request_meta(),
                "name": "workspace_get",
                "arguments": {"workspace_id": "x".repeat(protocol::MAX_TOOL_INPUT_BYTES)}
            }
        }),
    )
    .await;
    assert_eq!(
        oversized_input["result"]["structuredContent"]["code"],
        "INPUT_BUDGET_EXCEEDED"
    );

    let oversized_output = post(
        server.local_addr(),
        "tools/call",
        Some("certificate_overview"),
        json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "tools/call",
            "params": {
                "_meta": request_meta(),
                "name": "certificate_overview",
                "arguments": {}
            }
        }),
    )
    .await;
    assert_eq!(
        oversized_output["result"]["structuredContent"]["code"],
        "OUTPUT_BUDGET_EXCEEDED"
    );
    server.shutdown().await;
}
