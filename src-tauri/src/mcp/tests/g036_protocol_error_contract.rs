use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::{backend, protocol, request_meta, start_test_server};

const PRIVATE: &str = "DO_NOT_ECHO_PRIVATE_REQUEST_OR_LIBRARY_TEXT";

#[tokio::test]
async fn unsupported_http_method_returns_stable_sanitized_code() {
    assert_transport_error(
        "GET /mcp HTTP/1.1\r\nHost: proxy.test\r\nAuthorization: Bearer DO_NOT_ECHO_PRIVATE_REQUEST_OR_LIBRARY_TEXT\r\nConnection: close\r\n\r\n",
        "HTTP_METHOD_NOT_ALLOWED",
    )
    .await;
}

#[tokio::test]
async fn unknown_http_path_returns_stable_sanitized_code() {
    assert_transport_error(
        "POST /private/DO_NOT_ECHO_PRIVATE_REQUEST_OR_LIBRARY_TEXT HTTP/1.1\r\nHost: proxy.test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP_PATH_NOT_FOUND",
    )
    .await;
}

#[tokio::test]
async fn oversized_http_body_returns_stable_sanitized_code() {
    let body = PRIVATE.repeat((2 * 1024 * 1024 / PRIVATE.len()) + 2);
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: proxy.test\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: {}\r\nMcp-Method: tools/call\r\nMcp-Name: application_snapshot\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        protocol::PROTOCOL_VERSION,
        body.len()
    );
    assert_transport_error(&request, "HTTP_BODY_TOO_LARGE").await;
}

#[tokio::test]
async fn malformed_json_body_returns_stable_sanitized_code() {
    let body = format!("{{\"jsonrpc\":\"2.0\",\"private\":\"{PRIVATE}\"");
    let request = tool_http_request(&body, true);
    assert_transport_error(&request, "HTTP_MALFORMED").await;
}

#[tokio::test]
async fn invalid_json_rpc_envelope_returns_stable_sanitized_protocol_code() {
    let body = json!({
        "jsonrpc":"1.0",
        "id":41,
        "method":"tools/call",
        "params":{"_meta":request_meta(),"name":"application_snapshot","arguments":{"private":PRIVATE}}
    })
    .to_string();
    let request = tool_http_request(&body, true);
    assert_transport_error(&request, "MCP_PROTOCOL_INVALID").await;
}

#[tokio::test]
async fn missing_stateless_metadata_returns_stable_sanitized_protocol_code() {
    let body = json!({
        "jsonrpc":"2.0",
        "id":42,
        "method":"tools/call",
        "params":{"name":"application_snapshot","arguments":{"private":PRIVATE}}
    })
    .to_string();
    let request = tool_http_request(&body, false);
    assert_transport_error(&request, "MCP_PROTOCOL_INVALID").await;
}

async fn assert_transport_error(request: &str, expected_code: &str) {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let response = raw_http(server.local_addr(), request).await;
    server.shutdown().await;

    let (_, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("HTTP response has no body separator: {response:?}"));
    let value: Value = serde_json::from_str(body)
        .unwrap_or_else(|error| panic!("stable error must be JSON, got {body:?}: {error}"));
    assert_eq!(find_code(&value), Some(expected_code), "{response}");
    assert!(
        !response.contains(PRIVATE),
        "private request data leaked: {response}"
    );
    for forbidden in [
        "hyper",
        "rmcp",
        "serde",
        "tower",
        "backtrace",
        "panicked at",
    ] {
        assert!(
            !response.to_ascii_lowercase().contains(forbidden),
            "library/private implementation text leaked: {response}"
        );
    }
}

fn find_code(value: &Value) -> Option<&str> {
    match value {
        Value::Object(object) => object
            .get("code")
            .and_then(Value::as_str)
            .or_else(|| object.values().find_map(find_code)),
        Value::Array(values) => values.iter().find_map(find_code),
        _ => None,
    }
}

fn tool_http_request(body: &str, include_metadata_headers: bool) -> String {
    let metadata = if include_metadata_headers {
        format!(
            "MCP-Protocol-Version: {}\r\nMcp-Method: tools/call\r\nMcp-Name: application_snapshot\r\n",
            protocol::PROTOCOL_VERSION
        )
    } else {
        String::new()
    };
    format!(
        "POST /mcp HTTP/1.1\r\nHost: proxy.test\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\n{metadata}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn raw_http(address: std::net::SocketAddr, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect MCP server");
    if let Err(error) = stream.write_all(request.as_bytes()).await {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe,
            "write raw HTTP request: {error}"
        );
    }
    let mut response = String::new();
    if let Err(error) = stream.read_to_string(&mut response).await {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset,
            "read raw HTTP response: {error}"
        );
    }
    response
}
