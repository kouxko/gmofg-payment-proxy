use super::*;

mod http;

use http::post_tool_call_to_non_loopback;

#[derive(Debug)]
struct SnapshotCapabilitiesBackend;

#[async_trait]
impl ReadOnlyMcpBackend for SnapshotCapabilitiesBackend {
    async fn call_tool(&self, name: &str, _arguments: Value) -> ToolResult {
        Err(ToolFailure::not_found(format!("unexpected tool: {name}")))
    }

    async fn call_tool_with_context(
        &self,
        name: &str,
        _arguments: Value,
        context: McpCallContext,
    ) -> ToolResult {
        assert_eq!(name, "mcp_environment_capabilities");
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
        let capabilities = context.transport_capabilities;
        Ok(environment_capabilities_output(
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
            },
        ))
    }

    async fn read_resource(&self, _uri: &str) -> ToolResult {
        Err(ToolFailure::not_found("unexpected resource"))
    }
}

#[tokio::test]
async fn live_capabilities_are_the_exact_immutable_server_bind_snapshot() {
    let server = start_test_server(Arc::new(SnapshotCapabilitiesBackend))
        .await
        .expect("bind MCP server");
    let snapshot = server.transport_capabilities();
    let expected = environment_capabilities_output(transport_projection(&snapshot));
    let response = post(
        server.local_addr(),
        "tools/call",
        Some("mcp_environment_capabilities"),
        tool_call(1, "mcp_environment_capabilities", &json!({})),
    )
    .await;

    assert_eq!(response["result"]["isError"], false, "{response}");
    let actual = &response["result"]["structuredContent"];
    assert_eq!(actual, &expected);
    assert_eq!(actual["ipv4"]["available"], snapshot.ipv4().available());
    assert_eq!(
        actual["ipv4"]["bind_address"],
        snapshot.ipv4().bind_address()
    );
    assert_eq!(actual["ipv4"]["port"], snapshot.ipv4().port());
    assert_eq!(actual["ipv6"]["available"], snapshot.ipv6().available());
    assert_eq!(
        actual["ipv6"]["bind_address"],
        snapshot.ipv6().bind_address()
    );
    assert_eq!(actual["ipv6"]["port"], snapshot.ipv6().port());
    assert_eq!(actual["plaintext_http"], true);
    assert_eq!(actual["authentication"], "none");
    assert_eq!(actual["read_budgets"]["input_bytes"], 262_144);
    assert_eq!(actual["read_budgets"]["output_bytes"], 8_388_608);
    assert_eq!(actual["read_budgets"]["deadline_ms"], 8_000);
    assert_eq!(actual["write_budgets"]["create_deadline_ms"], 30_000);
    server.shutdown().await;
}

#[test]
fn ipv6_bind_outcomes_place_warnings_only_at_root_and_ipv6_binding() {
    for (available, warning) in [
        (true, None),
        (true, Some("ipv6_dual_stack_covered")),
        (false, Some("ipv6_unsupported")),
        (false, Some("IPV6_DEGRADED")),
    ] {
        let warnings = warning.into_iter().collect::<Vec<_>>();
        let output = environment_capabilities_output(EnvironmentTransportProjection {
            endpoint: "http://0.0.0.0:17653/mcp".to_owned(),
            ipv4: EnvironmentIpBindingProjection {
                available: true,
                bind_address: "0.0.0.0",
                port: 17_653,
                warning_codes: vec![],
            },
            ipv6: EnvironmentIpBindingProjection {
                available,
                bind_address: "[::]",
                port: 17_653,
                warning_codes: warnings.clone(),
            },
            warnings: warnings.clone(),
        });

        assert_eq!(output["warnings"], json!(warnings));
        assert_eq!(output["ipv4"]["warning_codes"], json!([]));
        assert_eq!(output["ipv6"]["warning_codes"], output["warnings"]);
        assert_eq!(output["ipv6"]["available"], available);
    }
}

#[tokio::test]
async fn ipv4_bind_failure_is_fatal_instead_of_starting_ipv6_only() {
    let _guard = production_bind_guard().await;
    let blocker = tokio::net::TcpListener::bind(MCP_ADDRESS)
        .await
        .expect("reserve production IPv4 MCP address for deterministic failure");
    let error = ReadOnlyMcpServer::start(backend())
        .await
        .expect_err("IPv4 bind failure must fail startup");

    assert!(
        error.to_string().starts_with("IPV4_BIND_FAILED:"),
        "{error}"
    );
    drop(blocker);
}

#[tokio::test]
async fn production_bind_is_reachable_on_current_platform_interfaces_without_false_availability() {
    let _guard = production_bind_guard().await;
    let reached = Arc::new(std::sync::Mutex::new(Vec::new()));
    let server = ReadOnlyMcpServer::start(Arc::new(InterfaceMarkerBackend {
        reached: Arc::clone(&reached),
    }))
    .await
    .expect("bind production MCP listeners");
    tokio::task::yield_now().await;
    let capabilities = server.transport_capabilities();

    assert_eq!(capabilities.ipv4().bind_address(), "0.0.0.0");
    assert!(capabilities.ipv4().available());
    let non_loopback = current_non_loopback_ipv4();
    let address = std::net::SocketAddr::new(non_loopback.into(), capabilities.ipv4().port());
    let calls = [
        ("mcp_environment_capabilities", json!({})),
        (
            "environment_candidate_create",
            json!({"candidate": full_candidate()}),
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
    for (id, (name, arguments)) in calls.iter().enumerate() {
        let response =
            post_tool_call_to_non_loopback(address, name, tool_call(id + 1, name, arguments)).await;
        assert_eq!(
            response["result"]["structuredContent"]["code"], "INTERFACE_BACKEND_REACHED",
            "{name} must reach the typed backend through the non-loopback interface: {response}"
        );
    }
    assert_eq!(
        *reached.lock().expect("interface backend call record"),
        calls
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect::<Vec<_>>()
    );

    let ipv6_connection = tokio::time::timeout(
        Duration::from_secs(1),
        tokio::net::TcpStream::connect((std::net::Ipv6Addr::LOCALHOST, capabilities.ipv6().port())),
    )
    .await;
    assert_eq!(
        ipv6_connection.is_ok_and(|result| result.is_ok()),
        capabilities.ipv6().available(),
        "reported IPv6 availability must equal actual current-platform reachability"
    );
    server.shutdown().await;
}

#[derive(Debug)]
struct InterfaceMarkerBackend {
    reached: Arc<std::sync::Mutex<Vec<String>>>,
}

#[async_trait]
impl ReadOnlyMcpBackend for InterfaceMarkerBackend {
    async fn call_tool(&self, name: &str, _arguments: Value) -> ToolResult {
        self.reached
            .lock()
            .expect("interface backend call record")
            .push(name.to_owned());
        Err(ToolFailure {
            code: "INTERFACE_BACKEND_REACHED".to_owned(),
            message: "interface backend reached".to_owned(),
            details: None,
        })
    }

    async fn read_resource(&self, _uri: &str) -> ToolResult {
        Err(ToolFailure::not_found("unexpected resource"))
    }
}

#[derive(Debug)]
struct MarkerBackend;

#[async_trait]
impl ReadOnlyMcpBackend for MarkerBackend {
    async fn call_tool(&self, _name: &str, _arguments: Value) -> ToolResult {
        Err(ToolFailure {
            code: "BACKEND_REACHED".to_owned(),
            message: "backend reached".to_owned(),
            details: None,
        })
    }

    async fn read_resource(&self, _uri: &str) -> ToolResult {
        Err(ToolFailure::not_found("unexpected resource"))
    }
}

#[tokio::test]
async fn create_accepts_more_than_read_budget_but_rejects_more_than_one_mibibyte() {
    let server = start_test_server(Arc::new(MarkerBackend))
        .await
        .expect("bind MCP server");
    let exact = create_arguments_with_logical_size(1_048_576);
    assert_eq!(serde_json::to_vec(&exact).unwrap().len(), 1_048_576);
    let accepted = post(
        server.local_addr(),
        "tools/call",
        Some("environment_candidate_create"),
        tool_call(2, "environment_candidate_create", &exact),
    )
    .await;
    assert_eq!(
        accepted["result"]["structuredContent"]["code"], "BACKEND_REACHED",
        "create must not inherit the 256 KiB read budget: {accepted}"
    );

    let oversized = create_arguments_with_logical_size(1_048_577);
    assert_eq!(serde_json::to_vec(&oversized).unwrap().len(), 1_048_577);
    let rejected = post(
        server.local_addr(),
        "tools/call",
        Some("environment_candidate_create"),
        tool_call(3, "environment_candidate_create", &oversized),
    )
    .await;
    assert_eq!(
        rejected["result"]["structuredContent"]["code"],
        "MCP_TOOL_ARGUMENTS_INVALID"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn status_cancel_and_apply_reject_more_than_sixteen_kibibytes() {
    let server = start_test_server(Arc::new(MarkerBackend))
        .await
        .expect("bind MCP server");
    for (id, name, extra) in [
        (4, "environment_candidate_status", json!({})),
        (5, "environment_candidate_cancel", json!({})),
        (
            6,
            "environment_candidate_apply",
            json!({"confirmation_token":"token-test"}),
        ),
    ] {
        let exact = string_arguments_with_logical_size(&extra, "candidate_id", 16_384);
        assert_eq!(serde_json::to_vec(&exact).unwrap().len(), 16_384);
        let accepted = post(
            server.local_addr(),
            "tools/call",
            Some(name),
            tool_call(id, name, &exact),
        )
        .await;
        assert_eq!(
            accepted["result"]["structuredContent"]["code"], "BACKEND_REACHED",
            "{name} exact boundary: {accepted}"
        );

        let oversized = string_arguments_with_logical_size(&extra, "candidate_id", 16_385);
        assert_eq!(serde_json::to_vec(&oversized).unwrap().len(), 16_385);
        let rejected = post(
            server.local_addr(),
            "tools/call",
            Some(name),
            tool_call(id + 100, name, &oversized),
        )
        .await;
        assert_eq!(
            rejected["result"]["structuredContent"]["code"], "MCP_TOOL_ARGUMENTS_INVALID",
            "{name} N+1 boundary: {rejected}"
        );
    }
    server.shutdown().await;
}

#[derive(Debug)]
struct SizedOutputBackend {
    logical_bytes: usize,
}

#[async_trait]
impl ReadOnlyMcpBackend for SizedOutputBackend {
    async fn call_tool(&self, _name: &str, _arguments: Value) -> ToolResult {
        let mut failure = ToolFailure {
            code: "BACKEND_ERROR".to_owned(),
            message: "bounded public error".to_owned(),
            details: Some(json!({"payload":""})),
        };
        let base = serde_json::to_vec(&failure.as_value()).unwrap().len();
        failure.details = Some(json!({"payload":"x".repeat(self.logical_bytes - base)}));
        assert_eq!(
            serde_json::to_vec(&failure.as_value()).unwrap().len(),
            self.logical_bytes
        );
        Err(failure)
    }

    async fn read_resource(&self, _uri: &str) -> ToolResult {
        Err(ToolFailure::not_found("unexpected resource"))
    }
}

#[tokio::test]
async fn environment_outputs_are_capped_at_one_mibibyte_while_reads_keep_eight_mibibytes() {
    for (id, name, arguments, budget) in [
        (7, "mcp_environment_capabilities", json!({}), 8_388_608),
        (
            8,
            "environment_candidate_create",
            json!({"candidate":full_candidate()}),
            1_048_576,
        ),
        (
            9,
            "environment_candidate_status",
            json!({"candidate_id":"candidate-test"}),
            1_048_576,
        ),
        (
            10,
            "environment_candidate_cancel",
            json!({"candidate_id":"candidate-test"}),
            1_048_576,
        ),
        (
            11,
            "environment_candidate_apply",
            json!({"candidate_id":"candidate-test","confirmation_token":"token-test"}),
            1_048_576,
        ),
    ] {
        let exact_server = start_test_server(Arc::new(SizedOutputBackend {
            logical_bytes: budget,
        }))
        .await
        .expect("bind exact-output MCP server");
        let exact = post(
            exact_server.local_addr(),
            "tools/call",
            Some(name),
            tool_call(id, name, &arguments),
        )
        .await;
        assert_eq!(
            exact["result"]["structuredContent"]["code"], "BACKEND_ERROR",
            "{name} exact output boundary: {exact}"
        );
        exact_server.shutdown().await;

        let oversized_server = start_test_server(Arc::new(SizedOutputBackend {
            logical_bytes: budget + 1,
        }))
        .await
        .expect("bind oversized-output MCP server");
        let oversized = post(
            oversized_server.local_addr(),
            "tools/call",
            Some(name),
            tool_call(id + 100, name, &arguments),
        )
        .await;
        assert_eq!(
            oversized["result"]["structuredContent"]["code"], "MCP_PROTOCOL_INVALID",
            "{name} N+1 output boundary: {oversized}"
        );
        oversized_server.shutdown().await;
    }
}

fn current_non_loopback_ipv4() -> std::net::Ipv4Addr {
    let socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))
        .expect("bind IPv4 route probe");
    socket
        .connect((std::net::Ipv4Addr::new(192, 0, 2, 1), 9))
        .expect("select current IPv4 route");
    let address = match socket.local_addr().expect("current IPv4 route").ip() {
        std::net::IpAddr::V4(address) => address,
        std::net::IpAddr::V6(_) => unreachable!("IPv4 UDP socket returned IPv6"),
    };
    assert!(
        !address.is_loopback(),
        "current platform has no non-loopback IPv4 route"
    );
    assert!(!address.is_unspecified());
    address
}
