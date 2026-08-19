use std::{fmt::Write as _, sync::Arc};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::{
    backend::{ReadOnlyMcpBackend, ToolFailure, ToolResult},
    protocol, resources,
    server::start_test_server,
};

#[derive(Debug)]
struct FakeBackend;

#[async_trait]
impl ReadOnlyMcpBackend for FakeBackend {
    async fn call_tool(&self, name: &str, arguments: Value) -> ToolResult {
        if name == "application_snapshot" {
            Ok(json!({ "tool": name, "arguments": arguments }))
        } else {
            Err(ToolFailure::not_found(format!("Unknown tool: {name}")))
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

fn backend() -> Arc<dyn ReadOnlyMcpBackend> {
    Arc::new(FakeBackend)
}

#[test]
fn tool_catalog_is_read_only_and_covers_runtime_and_portable_protocol_data() {
    let tools = protocol::tools();
    let names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();

    for required in [
        "application_snapshot",
        "diagnostics_query",
        "android_network_endpoints",
        "certificate_overview",
        "workspace_protocol_rule_list",
        "protocol_package_catalog",
        "protocol_package_detail",
        "protocol_package_usage",
    ] {
        assert!(names.contains(&required), "missing {required}");
    }
    assert!(tools.iter().all(|tool| {
        tool.annotations
            .as_ref()
            .is_some_and(|annotation| annotation.read_only_hint == Some(true))
    }));
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
        resource.uri == resources::ISO8583_ARCHIVE_URI
            && resource.mime_type.as_deref() == Some("application/zip")
    }));
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
async fn missing_current_protocol_metadata_is_rejected_before_tool_execution() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let response = post_without_protocol_metadata(
        server.local_addr(),
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    assert!(response.get("error").is_some(), "{response}");
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
