use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::Notify,
};

static PRODUCTION_BIND_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

use super::{backend, post, protocol, request_meta, start_test_server};
use crate::mcp::{
    backend::{McpBackend, McpCallContext, ToolFailure, ToolResult},
    environment_contract::{
        EnvironmentIpBindingProjection, EnvironmentTransportProjection,
        environment_capabilities_output,
    },
    server::{MCP_ADDRESS, McpServer},
};

mod application_lifecycle;
mod limits;

async fn production_bind_guard() -> tokio::sync::MutexGuard<'static, ()> {
    PRODUCTION_BIND_LOCK.lock().await
}

#[derive(Debug)]
struct DelayedBackend {
    entered: Notify,
    delay: Duration,
}

#[async_trait]
impl McpBackend for DelayedBackend {
    async fn call_tool(&self, _name: &str, _arguments: Value) -> ToolResult {
        self.entered.notify_one();
        tokio::time::sleep(self.delay).await;
        Err(ToolFailure {
            code: "BACKEND_COMPLETED".to_owned(),
            message: "backend completed".to_owned(),
            details: None,
        })
    }

    async fn read_resource(&self, _uri: &str) -> ToolResult {
        Err(ToolFailure::not_found("unexpected resource"))
    }
}

#[tokio::test(start_paused = true)]
async fn create_has_no_outer_deadline_competing_with_application_ownership() {
    let backend = Arc::new(DelayedBackend {
        entered: Notify::new(),
        delay: Duration::from_secs(31),
    });
    let server = start_test_server(backend.clone())
        .await
        .expect("bind MCP server");
    let address = server.local_addr();
    let request = tokio::spawn(async move {
        raw_tool_call(
            address,
            "environment_candidate_create",
            tool_call(
                12,
                "environment_candidate_create",
                &json!({"candidate":full_candidate()}),
            ),
        )
        .await
    });
    backend.entered.notified().await;
    tokio::time::advance(Duration::from_secs(30)).await;
    tokio::task::yield_now().await;
    assert!(
        !request.is_finished(),
        "an outer 30-second timeout must not race Application create cleanup"
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    let response = request.await.expect("request task");

    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("BACKEND_COMPLETED"), "{response}");
    server.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn read_status_cancel_and_apply_enforce_their_eight_second_deadline() {
    for (id, name, arguments, expected_code) in [
        (
            13,
            "application_snapshot",
            json!({}),
            "TOOL_DEADLINE_EXCEEDED",
        ),
        (
            14,
            "environment_candidate_status",
            json!({"candidate_id":"candidate-test"}),
            "MCP_PROTOCOL_INVALID",
        ),
        (
            15,
            "environment_candidate_cancel",
            json!({"candidate_id":"candidate-test"}),
            "MCP_PROTOCOL_INVALID",
        ),
        (
            16,
            "environment_candidate_apply",
            json!({"candidate_id":"candidate-test","confirmation_token":"token-test"}),
            "MCP_PROTOCOL_INVALID",
        ),
    ] {
        let backend = Arc::new(DelayedBackend {
            entered: Notify::new(),
            delay: Duration::from_secs(9),
        });
        let server = start_test_server(backend.clone())
            .await
            .expect("bind MCP server");
        let address = server.local_addr();
        let request = tokio::spawn(async move {
            super::post(
                address,
                "tools/call",
                Some(name),
                tool_call(id, name, &arguments),
            )
            .await
        });
        backend.entered.notified().await;
        tokio::time::advance(Duration::from_secs(8)).await;
        let response = request.await.expect("request task");
        assert_eq!(
            response["result"]["structuredContent"]["code"], expected_code,
            "{name}: {response}"
        );
        server.shutdown().await;
    }
}

#[derive(Debug)]
struct CancellationBackend {
    entered: Notify,
    cancelled: AtomicBool,
}

#[async_trait]
impl McpBackend for CancellationBackend {
    async fn call_tool(&self, _name: &str, _arguments: Value) -> ToolResult {
        unreachable!("create must use context-aware dispatch")
    }

    async fn call_tool_with_context(
        &self,
        name: &str,
        _arguments: Value,
        context: McpCallContext,
    ) -> ToolResult {
        assert_eq!(name, "environment_candidate_create");
        self.entered.notify_one();
        context.request_cancellation.cancelled().await;
        self.cancelled.store(true, Ordering::SeqCst);
        Err(ToolFailure {
            code: "CANDIDATE_CANCELLED".to_owned(),
            message: "candidate cancelled".to_owned(),
            details: None,
        })
    }

    async fn read_resource(&self, _uri: &str) -> ToolResult {
        Err(ToolFailure::not_found("unexpected resource"))
    }
}

#[tokio::test]
async fn dropping_a_create_connection_cancels_the_backend_request_context() {
    let backend = Arc::new(CancellationBackend {
        entered: Notify::new(),
        cancelled: AtomicBool::new(false),
    });
    let server = start_test_server(backend.clone())
        .await
        .expect("bind MCP server");
    let mut stream = write_tool_call_without_reading(
        server.local_addr(),
        "environment_candidate_create",
        tool_call(
            17,
            "environment_candidate_create",
            &json!({"candidate":full_candidate()}),
        ),
    )
    .await;
    backend.entered.notified().await;
    stream.shutdown().await.expect("close client connection");
    drop(stream);

    tokio::time::timeout(Duration::from_secs(1), async {
        while !backend.cancelled.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("disconnect must cancel request context");
    server.shutdown().await;
}

#[derive(Debug)]
struct OwnedApplyBackend {
    entered: Notify,
    owned_completed: Arc<Notify>,
}

#[async_trait]
impl McpBackend for OwnedApplyBackend {
    async fn call_tool(&self, _name: &str, _arguments: Value) -> ToolResult {
        unreachable!("apply must use context-aware dispatch")
    }

    async fn call_tool_with_context(
        &self,
        name: &str,
        _arguments: Value,
        _context: McpCallContext,
    ) -> ToolResult {
        assert_eq!(name, "environment_candidate_apply");
        let completed = Arc::clone(&self.owned_completed);
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            completed.notify_one();
        });
        self.entered.notify_one();
        Ok(json!({
            "candidate_id":"candidate-test",
            "apply_task_id":"apply-task-test",
            "status":"apply_queued",
            "errors":[]
        }))
    }

    async fn read_resource(&self, _uri: &str) -> ToolResult {
        Err(ToolFailure::not_found("unexpected resource"))
    }
}

#[tokio::test]
async fn apply_owned_work_survives_client_disconnect_after_ack_enqueue() {
    let completed = Arc::new(Notify::new());
    let backend = Arc::new(OwnedApplyBackend {
        entered: Notify::new(),
        owned_completed: Arc::clone(&completed),
    });
    let server = start_test_server(backend.clone())
        .await
        .expect("bind MCP server");
    let mut stream = write_tool_call_without_reading(
        server.local_addr(),
        "environment_candidate_apply",
        tool_call(
            18,
            "environment_candidate_apply",
            &json!({"candidate_id":"candidate-test","confirmation_token":"token-test"}),
        ),
    )
    .await;
    backend.entered.notified().await;
    stream.shutdown().await.expect("close client connection");
    drop(stream);

    tokio::time::timeout(Duration::from_secs(1), completed.notified())
        .await
        .expect("Application-owned apply work must outlive the request connection");
    server.shutdown().await;
}

#[tokio::test]
async fn protocol_errors_remain_fail_closed_even_with_arbitrary_credentials() {
    let server = start_test_server(super::backend())
        .await
        .expect("bind MCP server");
    for (request, expected_status) in [
        (
            "GET /mcp HTTP/1.1\r\nHost: proxy.test\r\nAuthorization: Bearer anything\r\nConnection: close\r\n\r\n".to_owned(),
            "405 Method Not Allowed",
        ),
        (
            "POST /wrong HTTP/1.1\r\nHost: proxy.test\r\nX-API-Key: anything\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
            "404 Not Found",
        ),
    ] {
        let response = raw_http(server.local_addr(), &request).await;
        assert!(response.starts_with(&format!("HTTP/1.1 {expected_status}")), "{response}");
        assert!(!response.contains("Unauthorized"), "{response}");
        assert!(!response.contains("Forbidden"), "{response}");
    }
    server.shutdown().await;
}

#[tokio::test]
async fn environment_argument_errors_do_not_echo_private_input() {
    let server = start_test_server(super::backend())
        .await
        .expect("bind MCP server");
    let private = "DO_NOT_ECHO_PRIVATE_PASSWORD";
    let response = super::post(
        server.local_addr(),
        "tools/call",
        Some("environment_candidate_apply"),
        tool_call(
            19,
            "environment_candidate_apply",
            &json!({
                "candidate_id":"candidate-test",
                "confirmation_token":"",
                "private_password":private
            }),
        ),
    )
    .await;
    let rendered = response.to_string();

    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "MCP_TOOL_ARGUMENTS_INVALID"
    );
    assert!(!rendered.contains(private), "{rendered}");
    assert!(!rendered.contains("private_password"), "{rendered}");
    server.shutdown().await;
}

fn tool_call(id: usize, name: &str, arguments: &Value) -> Value {
    json!({
        "jsonrpc":"2.0",
        "id":id,
        "method":"tools/call",
        "params":{
            "_meta":request_meta(),
            "name":name,
            "arguments":arguments
        }
    })
}

fn full_candidate() -> Value {
    serde_json::from_slice(include_bytes!(
        "fixtures/environment_configuration_candidate_v1/full-shape.json"
    ))
    .expect("canonical candidate fixture")
}

fn transport_projection(
    capabilities: &crate::mcp::server::McpTransportCapabilities,
) -> EnvironmentTransportProjection {
    let ip = |binding: &crate::mcp::server::McpIpCapability| EnvironmentIpBindingProjection {
        available: binding.available(),
        bind_address: binding.bind_address(),
        port: binding.port(),
        warning_codes: binding
            .warning_codes()
            .iter()
            .copied()
            .map(crate::mcp::server::McpTransportWarningCode::as_str)
            .collect(),
    };
    EnvironmentTransportProjection {
        endpoint: format!(
            "http://{}:{}/mcp",
            capabilities.ipv4().bind_address(),
            capabilities.ipv4().port()
        ),
        ipv4: ip(capabilities.ipv4()),
        ipv6: ip(capabilities.ipv6()),
        warnings: capabilities
            .warnings()
            .iter()
            .copied()
            .map(crate::mcp::server::McpTransportWarningCode::as_str)
            .collect(),
    }
}

fn create_arguments_with_logical_size(logical_bytes: usize) -> Value {
    let mut candidate = full_candidate();
    candidate["materials"]["certificates"][0]["content"] = json!("");
    let empty = json!({"candidate":candidate});
    let base = serde_json::to_vec(&empty).unwrap().len();
    let mut candidate = empty["candidate"].clone();
    candidate["materials"]["certificates"][0]["content"] = json!("A".repeat(logical_bytes - base));
    json!({"candidate":candidate})
}

fn string_arguments_with_logical_size(base: &Value, field: &str, logical_bytes: usize) -> Value {
    let mut object = base.as_object().cloned().expect("object");
    object.insert(field.to_owned(), json!(""));
    let empty = Value::Object(object.clone());
    let base = serde_json::to_vec(&empty).unwrap().len();
    object.insert(field.to_owned(), json!("x".repeat(logical_bytes - base)));
    Value::Object(object)
}

async fn raw_tool_call(address: std::net::SocketAddr, name: &str, body: Value) -> String {
    let mut stream = write_tool_call_without_reading(address, name, body).await;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read MCP response");
    response
}

async fn write_tool_call_without_reading(
    address: std::net::SocketAddr,
    name: &str,
    body: Value,
) -> tokio::net::TcpStream {
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect MCP server");
    let body = body.to_string();
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: proxy.test\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: {}\r\nMcp-Method: tools/call\r\nMcp-Name: {name}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        protocol::PROTOCOL_VERSION,
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write MCP request");
    stream
}

async fn raw_http(address: std::net::SocketAddr, request: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect MCP server");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write raw request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read raw response");
    response
}
