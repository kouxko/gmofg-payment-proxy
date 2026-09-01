use std::{fmt::Write as _, sync::Arc};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::{
    backend::{McpBackend, ToolFailure, ToolResult},
    protocol, resources,
    server::start_test_server,
};

mod environment_configuration_contract;
mod environment_configuration_schema_contract;
mod g036_adapter_transport_contract;
mod g036_behavior_contract;
mod g036_protocol_error_contract;
mod protocol_contract;

#[derive(Debug)]
struct FakeBackend;

#[async_trait]
impl McpBackend for FakeBackend {
    async fn call_tool(&self, name: &str, arguments: Value) -> ToolResult {
        match name {
            "application_snapshot" => Ok(json!({ "tool": name, "arguments": arguments })),
            "workspace_list" | "settings_get" | "android_runtime_owner_list" => Ok(json!([])),
            "diagnostics_query" => Ok(json!({"rows": []})),
            "certificate_overview" => Ok(json!({
                "payload": "x".repeat(protocol::MAX_LOGICAL_OUTPUT_BYTES)
            })),
            _ => Err(ToolFailure::not_found(format!("Unknown tool: {name}"))),
        }
    }

    async fn read_resource(&self, uri: &str) -> ToolResult {
        if uri == resources::AUTHORING_GUIDE_URI {
            Ok(json!({
                "uri": uri,
                "mimeType": "text/markdown",
                "text": "guide"
            }))
        } else {
            Err(ToolFailure::not_found("unknown resource"))
        }
    }
}

fn backend() -> Arc<dyn McpBackend> {
    Arc::new(FakeBackend)
}

#[test]
fn tool_catalog_preserves_the_existing_read_only_runtime_and_portable_protocol_tools() {
    let tools = protocol::tools();
    let read_tools = tools
        .iter()
        .filter(|tool| {
            tool.annotations
                .as_ref()
                .is_some_and(|annotation| annotation.read_only_hint == Some(true))
        })
        .collect::<Vec<_>>();
    let names = read_tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();

    for required in [
        "application_snapshot",
        "application_log_query",
        "application_log_get",
        "reproduction_report",
        "diagnostics_query",
        "external_package_service_status",
        "android_network_endpoints",
        "android_runtime_owner_list",
        "certificate_overview",
        "rule_list",
        "rule_get",
        "workspace_rule_list",
        "protocol_package_catalog",
        "protocol_package_detail",
        "protocol_package_usage",
    ] {
        assert!(names.contains(&required), "missing {required}");
    }
    assert_eq!(
        read_tools.len(),
        36,
        "unified reads plus capabilities/status"
    );
    for forbidden in [
        "save", "create", "delete", "clear", "reset", "start", "stop", "import", "export",
        "execute", "write", "sql", "shell",
    ] {
        assert!(
            names.iter().all(|name| !name.contains(forbidden)),
            "forbidden capability in MCP catalog: {forbidden}"
        );
    }
}

#[test]
fn resources_include_authoring_manifest_and_official_zip() {
    let resources = resources::list();
    assert!(
        resources
            .iter()
            .any(|resource| resource.uri == resources::AUTHORING_GUIDE_URI)
    );
    assert!(
        resources
            .iter()
            .any(|resource| resource.uri == resources::HOST_API_URI)
    );
    assert!(
        resources
            .iter()
            .any(|resource| resource.uri == resources::SOCKET_AUTHORING_URI)
    );
    assert!(resources.iter().any(|resource| {
        resource.uri == resources::CERTIFICATE_CONCEPTS_URI
            && resource.mime_type.as_deref() == Some("text/markdown")
    }));
    assert!(resources.iter().any(|resource| {
        resource.uri == resources::APP_INTEGRATION_GUIDE_URI
            && resource.mime_type.as_deref() == Some("text/markdown")
    }));
    assert!(resources.iter().any(|resource| {
        resource.uri == resources::EXTERNAL_PACKAGE_INTEGRATION_GUIDE_URI
            && resource.mime_type.as_deref() == Some("text/markdown")
    }));
    assert!(resources.iter().any(|resource| {
        resource.uri == resources::DIAGNOSTIC_ARCHITECTURE_URI
            && resource.mime_type.as_deref() == Some("text/markdown")
    }));
    assert!(resources.iter().any(|resource| {
        resource.uri == resources::TOOL_REFERENCE_URI
            && resource.mime_type.as_deref() == Some("text/markdown")
            && resource.title.as_deref()
                == Some("Complete MCP tool reference: 36 reads and 5 environment tools")
    }));
    assert!(resources.iter().any(|resource| {
        resource.uri == resources::VALIDATION_PLAYBOOK_URI
            && resource.mime_type.as_deref() == Some("text/markdown")
    }));
    assert!(resources.iter().any(|resource| {
        resource.uri == resources::ISO8583_ARCHIVE_URI
            && resource.mime_type.as_deref() == Some("application/zip")
    }));
}

#[test]
fn validation_playbook_keeps_advice_evidence_based_and_fail_closed() {
    let (_, guide) =
        resources::text(resources::VALIDATION_PLAYBOOK_URI).expect("validation playbook");
    for required in [
        "已观测事实",
        "推断",
        "未知",
        "NOT_RUN",
        "exchange_observation_get",
        "verify_hostname=false",
        "runtime epoch",
        "Frame → Decode → Display → Rules → Encode",
        "HTTP_MOCK_DRAFT",
    ] {
        assert!(guide.contains(required), "playbook is missing {required}");
    }
}

#[test]
fn tool_reference_names_every_public_tool_and_explains_the_result_contract() {
    let (_, reference) = resources::text(resources::TOOL_REFERENCE_URI).expect("tool reference");
    for tool in protocol::tools() {
        assert!(
            reference.contains(&format!("`{}`", tool.name)),
            "reference is missing {}",
            tool.name
        );
    }
    for required in [
        "成功结果",
        "错误结果",
        "additionalProperties",
        "ExchangeObservationStore",
    ] {
        assert!(
            reference.contains(required),
            "reference is missing {required}"
        );
    }
}

#[test]
fn diagnostic_architecture_resource_maps_evidence_to_code_and_reproduction_tools() {
    let (_, guide) = resources::text(resources::DIAGNOSTIC_ARCHITECTURE_URI)
        .expect("diagnostic architecture guide");

    for required in [
        "application_log_query",
        "application_log_get",
        "diagnostics_query",
        "exchange_observation_get",
        "reproduction_report",
        "ListenerRuntime",
        "external_package",
        "src-tauri/crates/application",
        "src-tauri/crates/infrastructure",
        "src-tauri/crates/proxy",
    ] {
        assert!(guide.contains(required), "guide is missing {required}");
    }
}

#[test]
fn external_package_guide_explains_connection_runtime_diagnostics_and_read_only_boundaries() {
    let (_, guide) = resources::text(resources::EXTERNAL_PACKAGE_INTEGRATION_GUIDE_URI)
        .expect("external package guide");

    for required in [
        "external_package_service_status",
        "ws://",
        "/packages",
        "package.register",
        "hooks.upstream",
        "document.downstream",
        "diagnostics_query",
        "exchange_observation_query",
        "protocol_package_detail",
        "max_body_bytes",
        "canonical padded Base64",
        "只读",
    ] {
        assert!(guide.contains(required), "guide is missing {required}");
    }
}

#[test]
fn every_text_resource_resolves_with_its_declared_mime_type() {
    for resource in resources::list()
        .into_iter()
        .filter(|resource| resource.uri != resources::ISO8583_ARCHIVE_URI)
    {
        let (mime_type, text) = resources::text(&resource.uri).expect("listed text resource");
        assert_eq!(resource.mime_type.as_deref(), Some(mime_type));
        assert!(!text.is_empty());
    }
    assert!(resources::text(resources::ISO8583_ARCHIVE_URI).is_none());
    assert!(resources::text("intercept-proxy://unknown").is_none());
}

#[tokio::test]
async fn current_protocol_discovery_and_tool_call_use_stateless_metadata() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let discover = post(
        server.local_addr(),
        "server/discover",
        None,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "server/discover",
            "params": {"_meta": request_meta()}
        }),
    )
    .await;
    assert_eq!(discover["result"]["resultType"], "complete");
    assert_eq!(
        discover["result"]["supportedVersions"],
        json!([protocol::PROTOCOL_VERSION])
    );

    let called = post(
        server.local_addr(),
        "tools/call",
        Some("application_snapshot"),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "_meta": request_meta(),
                "name": "application_snapshot",
                "arguments": {}
            }
        }),
    )
    .await;
    assert_eq!(called["result"]["resultType"], "complete");
    assert_eq!(called["result"]["isError"], false);
    assert_eq!(
        called["result"]["structuredContent"]["tool"],
        "application_snapshot"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn closed_input_schema_is_enforced_before_backend_execution() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let response = post(
        server.local_addr(),
        "tools/call",
        Some("application_snapshot"),
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "_meta": request_meta(),
                "name": "application_snapshot",
                "arguments": {"unexpected": true}
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
            .is_some_and(|message| message.contains("unexpected"))
    );
    server.shutdown().await;
}

#[tokio::test]
async fn missing_current_protocol_metadata_is_rejected_before_tool_execution() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let response = post_without_protocol_metadata(
        server.local_addr(),
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    assert_eq!(
        response,
        json!({
            "code": "MCP_PROTOCOL_INVALID",
            "message": "MCP protocol request is invalid",
            "details": null
        })
    );
    server.shutdown().await;
}

fn request_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": protocol::PROTOCOL_VERSION,
        "io.modelcontextprotocol/clientInfo": {"name": "test-client", "version": "1.0.0"},
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

async fn post(
    address: std::net::SocketAddr,
    method: &str,
    name: Option<&str>,
    body: Value,
) -> Value {
    let mut headers = format!(
        "MCP-Protocol-Version: {}\r\nMcp-Method: {method}\r\n",
        protocol::PROTOCOL_VERSION
    );
    if let Some(name) = name {
        write!(headers, "Mcp-Name: {name}\r\n").expect("writing to String cannot fail");
    }
    post_raw(address, &headers, body).await
}

async fn post_without_protocol_metadata(address: std::net::SocketAddr, body: Value) -> Value {
    post_raw(address, "Mcp-Method: tools/list\r\n", body).await
}

async fn post_raw(address: std::net::SocketAddr, extra_headers: &str, body: Value) -> Value {
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect MCP server");
    let body = body.to_string();
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read response");
    let (_, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("HTTP response body missing: {response:?}"));
    serde_json::from_str(body)
        .unwrap_or_else(|error| panic!("JSON response: {error}; raw response: {response:?}"))
}
