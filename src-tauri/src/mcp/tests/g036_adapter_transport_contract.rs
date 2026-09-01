use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::{backend, protocol, request_meta, start_test_server};
use crate::mcp::server::{Ipv6BindOutcome, McpListenerBinder, bind_production_listeners_with};

const ENVIRONMENT_TOOL_NAMES: [&str; 5] = [
    "mcp_environment_capabilities",
    "environment_candidate_create",
    "environment_candidate_status",
    "environment_candidate_cancel",
    "environment_candidate_apply",
];

const EXISTING_READ_TOOL_NAMES: [&str; 34] = [
    "application_snapshot",
    "application_log_query",
    "application_log_get",
    "exchange_observation_query",
    "exchange_observation_get",
    "reproduction_report",
    "settings_get",
    "workspace_list",
    "workspace_get",
    "entry_overview",
    "entry_status_list",
    "diagnostics_query",
    "diagnose_recent_failures",
    "external_package_service_status",
    "android_adb_get",
    "android_device_list",
    "android_package_list",
    "android_package_get",
    "android_profile_list",
    "android_profile_get",
    "android_network_status",
    "android_runtime_owner_list",
    "android_network_endpoints",
    "certificate_overview",
    "workspace_certificate_overview",
    "http_capture_query",
    "http_capture_get",
    "rule_list",
    "rule_get",
    "workspace_rule_list",
    "protocol_package_list",
    "protocol_package_catalog",
    "protocol_package_detail",
    "protocol_package_usage",
];

#[test]
fn active_catalog_exposes_exactly_the_five_environment_configuration_tools() {
    let active = protocol::tools();
    let actual = active
        .iter()
        .map(|tool| tool.name.as_ref())
        .filter(|name| ENVIRONMENT_TOOL_NAMES.contains(name))
        .collect::<BTreeSet<_>>();

    assert_eq!(actual, ENVIRONMENT_TOOL_NAMES.into_iter().collect());
    assert_eq!(active.len(), 39, "34 existing reads plus five writes");
}

#[test]
fn active_catalog_preserves_every_existing_read_tool() {
    let actual = protocol::tools()
        .into_iter()
        .filter(|tool| EXISTING_READ_TOOL_NAMES.contains(&tool.name.as_ref()))
        .map(|tool| tool.name.into_owned())
        .collect::<BTreeSet<_>>();
    let expected = EXISTING_READ_TOOL_NAMES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
}

#[test]
fn active_environment_tools_publish_the_exact_mutation_annotations() {
    let expected = BTreeMap::from([
        ("mcp_environment_capabilities", (true, false, true)),
        ("environment_candidate_create", (false, false, false)),
        ("environment_candidate_status", (true, false, true)),
        ("environment_candidate_cancel", (false, true, true)),
        ("environment_candidate_apply", (false, true, false)),
    ]);

    let tools = protocol::tools()
        .into_iter()
        .filter(|tool| expected.contains_key(tool.name.as_ref()))
        .collect::<Vec<_>>();
    assert_eq!(
        tools.len(),
        expected.len(),
        "all five annotations are active"
    );
    for tool in tools {
        let annotation = tool.annotations.expect("environment annotation");
        let (read_only, destructive, idempotent) = expected[tool.name.as_ref()];
        assert_eq!(annotation.read_only_hint, Some(read_only), "{}", tool.name);
        assert_eq!(
            annotation.destructive_hint,
            Some(destructive),
            "{}",
            tool.name
        );
        assert_eq!(
            annotation.idempotent_hint,
            Some(idempotent),
            "{}",
            tool.name
        );
    }
}

#[tokio::test]
async fn every_environment_tool_reaches_the_live_backend_dispatch() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "fixtures/environment_configuration_candidate_v1/full-shape.json"
    ))
    .expect("canonical candidate fixture");
    let calls = [
        ("mcp_environment_capabilities", json!({})),
        (
            "environment_candidate_create",
            json!({"candidate": fixture}),
        ),
        (
            "environment_candidate_status",
            json!({"candidate_id":"candidate-test"}),
        ),
        (
            "environment_candidate_cancel",
            json!({"candidate_id":"candidate-test"}),
        ),
        (
            "environment_candidate_apply",
            json!({"candidate_id":"candidate-test","confirmation_token":"token-test"}),
        ),
    ];

    for (id, (name, arguments)) in calls.into_iter().enumerate() {
        let response = super::post(
            server.local_addr(),
            "tools/call",
            Some(name),
            json!({
                "jsonrpc":"2.0",
                "id":id + 1,
                "method":"tools/call",
                "params":{
                    "_meta":request_meta(),
                    "name":name,
                    "arguments":arguments
                }
            }),
        )
        .await;
        assert_eq!(
            response["result"]["structuredContent"]["code"], "NOT_FOUND",
            "{name} must pass protocol/catalog validation and reach FakeBackend: {response}"
        );
    }
    server.shutdown().await;
}

#[test]
fn environment_dispatch_uses_a_typed_request_boundary() {
    let backend = include_str!("../backend.rs");
    let dispatch = include_str!("../backend/dispatch.rs");
    let combined = format!("{backend}\n{dispatch}");

    assert!(
        combined.contains("EnvironmentToolRequest"),
        "the write boundary must define a typed EnvironmentToolRequest"
    );
    assert!(
        combined.contains("call_environment_tool"),
        "environment tools must use a dedicated typed backend entry point"
    );
    for variant in ["Capabilities", "Create", "Status", "Cancel", "Apply"] {
        assert!(
            combined.contains(&format!("EnvironmentToolRequest::{variant}")),
            "missing typed environment dispatch variant {variant}"
        );
    }
}

#[test]
fn protocol_declares_exact_per_tool_logical_budgets_and_deadlines() {
    let source = include_str!("../protocol.rs").replace('_', "");
    for required in [
        "READINPUTBYTES:usize=256*1024",
        "READOUTPUTBYTES:usize=8*1024*1024",
        "READDEADLINE:Duration=Duration::fromsecs(8)",
        "CREATEINPUTBYTES:usize=1024*1024",
        "CREATEOUTPUTBYTES:usize=1024*1024",
        "CREATEDEADLINE:Duration=Duration::fromsecs(30)",
        "STATUSCANCELAPPLYINPUTBYTES:usize=16*1024",
        "STATUSCANCELAPPLYOUTPUTBYTES:usize=1024*1024",
        "STATUSCANCELAPPLYDEADLINE:Duration=Duration::fromsecs(8)",
    ] {
        assert!(
            source
                .split_whitespace()
                .collect::<String>()
                .contains(required),
            "missing exact per-tool protocol budget declaration: {required}"
        );
    }
}

#[test]
fn create_dispatch_propagates_the_rmcp_request_cancellation_token() {
    let protocol = include_str!("../protocol.rs");
    let backend = include_str!("../backend.rs");

    assert!(
        protocol.contains("context.ct"),
        "RequestContext.ct must be used"
    );
    assert!(
        backend.contains("request_cancellation: CancellationToken"),
        "typed create dispatch must accept the request cancellation token"
    );
    assert!(
        backend.contains("environment_candidate_run_validation")
            && backend.contains("request_cancellation"),
        "create must forward cancellation to Application validation"
    );
}

#[test]
fn server_source_has_ipv4_and_ipv6_all_interface_plaintext_no_auth_policy() {
    let source = include_str!("../server.rs");

    assert!(
        source.contains("0.0.0.0:17653"),
        "missing IPv4 all-interface bind"
    );
    assert!(
        source.contains("[::]:17653"),
        "missing IPv6 all-interface bind"
    );
    for forbidden in [
        "is_loopback()",
        "with_allowed_hosts",
        "with_allowed_origins",
        "Authorization",
        "X-API-Key",
    ] {
        assert!(
            !source.contains(forbidden),
            "plaintext no-auth transport must not gate requests with {forbidden}"
        );
    }
}

#[test]
fn mcp_backend_does_not_import_the_infrastructure_crate_directly() {
    let source = include_str!("../backend.rs");

    assert!(
        !source.contains("intercept_proxy_infrastructure"),
        "the MCP adapter must depend on an Application-facing port, not Infrastructure directly"
    );
}

#[tokio::test]
async fn production_binding_classifies_independent_ipv6_success() {
    assert_binding(
        DeterministicBinder::independent(),
        Ipv6BindOutcome::Independent,
        2,
    )
    .await;
}

#[tokio::test]
async fn production_binding_classifies_verified_dual_stack_coverage() {
    assert_binding(
        DeterministicBinder::dual_stack(),
        Ipv6BindOutcome::DualStackCovered,
        1,
    )
    .await;
}

#[tokio::test]
async fn production_binding_classifies_unsupported_ipv6_without_false_availability() {
    assert_binding(
        DeterministicBinder::ipv6_error(io::ErrorKind::Unsupported),
        Ipv6BindOutcome::Unsupported,
        1,
    )
    .await;
}

#[tokio::test]
async fn production_binding_classifies_degraded_ipv6_without_false_availability() {
    assert_binding(
        DeterministicBinder::ipv6_error(io::ErrorKind::AddrInUse),
        Ipv6BindOutcome::Degraded,
        1,
    )
    .await;
}

#[derive(Debug)]
struct DeterministicBinder {
    dual_stack: bool,
    ipv6_error: Option<io::ErrorKind>,
}

impl DeterministicBinder {
    const fn independent() -> Self {
        Self {
            dual_stack: false,
            ipv6_error: None,
        }
    }

    const fn dual_stack() -> Self {
        Self {
            dual_stack: true,
            ipv6_error: None,
        }
    }

    const fn ipv6_error(kind: io::ErrorKind) -> Self {
        Self {
            dual_stack: false,
            ipv6_error: Some(kind),
        }
    }
}

#[async_trait]
impl McpListenerBinder for DeterministicBinder {
    type Listener = &'static str;

    fn bind_ipv4(&self) -> io::Result<Self::Listener> {
        Ok("ipv4")
    }

    fn local_addr(&self, _listener: &Self::Listener) -> io::Result<SocketAddr> {
        Ok(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::UNSPECIFIED,
            17_653,
        )))
    }

    async fn probe_ipv4_listener_for_ipv6(&self, _listener: &Self::Listener, _port: u16) -> bool {
        self.dual_stack
    }

    fn bind_ipv6(&self) -> io::Result<Self::Listener> {
        self.ipv6_error
            .map_or(Ok("ipv6"), |kind| Err(io::Error::from(kind)))
    }
}

async fn assert_binding(
    binder: DeterministicBinder,
    expected: Ipv6BindOutcome,
    expected_listener_count: usize,
) {
    let binding = bind_production_listeners_with(&binder)
        .await
        .expect("IPv4 remains available");
    assert_eq!(binding.local_addr, "0.0.0.0:17653".parse().unwrap());
    assert_eq!(binding.ipv6, expected);
    assert_eq!(binding.listeners.len(), expected_listener_count);
}

#[tokio::test]
async fn syntactically_valid_host_origin_and_credentials_do_not_gate_mcp_protocol_handling() {
    let server = start_test_server(backend()).await.expect("bind MCP server");
    for headers in [
        "Host: proxy-admin.test\r\n",
        "Host: [2001:db8::10]:17653\r\n",
        "Host: proxy-admin.test\r\nOrigin: https://admin.example.test:8443\r\n",
        "Host: proxy-admin.test\r\nAuthorization: Bearer invalid\r\nX-API-Key: invalid\r\nCookie: session=invalid\r\n",
    ] {
        let response = raw_tools_list(server.local_addr(), headers).await;
        assert!(
            response.starts_with("HTTP/1.1 200"),
            "headers must reach MCP protocol handling without authorization gates: {response}"
        );
    }
    server.shutdown().await;
}

async fn raw_tools_list(address: std::net::SocketAddr, headers: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect MCP server");
    let body = json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"tools/list",
        "params":{"_meta":request_meta()}
    })
    .to_string();
    let request = format!(
        "POST /mcp HTTP/1.1\r\n{headers}Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\nMCP-Protocol-Version: {}\r\nMcp-Method: tools/list\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        protocol::PROTOCOL_VERSION,
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write MCP request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read MCP response");
    response
}
