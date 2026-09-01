use super::*;

fn assert_environment_commit_event(
    event: &intercept_proxy_application::UiEventEnvelope,
    terminal: &Value,
) {
    assert_eq!(
        event.entity_id.as_deref(),
        terminal["terminal_result"]["workspace_id"].as_str()
    );
    assert_eq!(
        event.entity_revision,
        terminal["terminal_result"]["revision"].as_u64()
    );
    assert!(matches!(
        event.payload,
        intercept_proxy_application::UiEventPayload::SnapshotRequired { ref reason }
            if reason == "environment_configuration_committed"
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn production_ports_commit_minimal_new_workspace_with_builtin_package_inventory() {
    let _guard = APPLICATION_HOST_LOCK.lock().await;
    let directory = TempDir::new().expect("temporary production Host data directory");
    let host = ApplicationHostBuilder::new(
        directory.path(),
        HostPlatformServices::new(Arc::new(TestSecrets), Arc::new(NoopDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .with_builtin_protocol_package(Arc::from(crate::BUILTIN_ISO8583_COMPONENT))
    .build()
    .await
    .expect("build production environment services");
    let application = host.application();
    let backend = Arc::new(ApplicationBackend::new(
        Arc::clone(&application),
        Arc::new(RuntimeLogStore::memory(32)),
        ExchangeObservationQueries::new(Arc::new(NoopObservations)),
    ));
    let server = start_test_server(backend)
        .await
        .expect("start production-services MCP server");

    let created = call(
        &server,
        301,
        "environment_candidate_create",
        json!({"candidate":minimal_candidate("Production Apply")}),
    )
    .await;
    assert_eq!(created["status"], "preview_ready", "{created}");
    let candidate_id = created["candidate_id"].as_str().expect("candidate id");
    let confirmation_token = created["confirmation_token"]
        .as_str()
        .expect("confirmation token");
    let event_cursor = application
        .app_bootstrap()
        .await
        .expect("read pre-commit event cursor")
        .event_cursor;
    let mut events = application
        .app_subscribe_events(event_cursor)
        .expect("subscribe before environment commit");

    let queued = call(
        &server,
        302,
        "environment_candidate_apply",
        json!({
            "candidate_id":candidate_id,
            "confirmation_token":confirmation_token,
        }),
    )
    .await;
    assert_eq!(queued["status"], "apply_queued", "{queued}");

    let terminal = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = call(
                &server,
                303,
                "environment_candidate_status",
                json!({"candidate_id":candidate_id}),
            )
            .await;
            if !matches!(
                status["status"].as_str(),
                Some("apply_queued" | "apply_in_progress")
            ) {
                return status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("production apply reaches terminal state");
    assert_eq!(terminal["status"], "committed", "{terminal}");
    assert_eq!(terminal["terminal_result"]["result"], "committed");
    let committed_event = tokio::time::timeout(Duration::from_secs(1), events.live.recv())
        .await
        .expect("successful environment commit publishes an application event")
        .expect("environment commit event remains subscribed");
    assert_environment_commit_event(&committed_event, &terminal);

    server.shutdown().await;
    host.shutdown().await.expect("shutdown production Host");

    let restarted = build_production_host(&directory).await;
    assert!(
        restarted
            .application()
            .workspace_list()
            .await
            .expect("reload committed Workspace")
            .iter()
            .any(|workspace| workspace.name == "Production Apply")
    );
    restarted.shutdown().await.expect("shutdown restarted Host");
}

#[tokio::test(flavor = "current_thread")]
async fn production_full_resource_candidate_requires_managed_package_online() {
    let _guard = APPLICATION_HOST_LOCK.lock().await;
    let directory = TempDir::new().expect("temporary production Host data directory");
    let host = build_production_host(&directory).await;
    let backend = Arc::new(ApplicationBackend::new(
        Arc::clone(&host.application()),
        Arc::new(RuntimeLogStore::memory(32)),
        ExchangeObservationQueries::new(Arc::new(NoopObservations)),
    ));
    let server = start_test_server(backend)
        .await
        .expect("start production-services MCP server");

    let created = call(
        &server,
        311,
        "environment_candidate_create",
        json!({"candidate":full_resource_candidate()}),
    )
    .await;
    assert_eq!(created["status"], "validation_failed", "{created}");
    assert_eq!(created["errors"][0]["code"], "EXTERNAL_PACKAGE_OFFLINE");
    assert_eq!(
        created["validation_layers"][3]["layer"],
        "package_projection"
    );
    assert_eq!(created["validation_layers"][3]["status"], "failed");
    assert!(created["confirmation_token"].is_null());
    assert!(created["preview"].is_null());

    server.shutdown().await;
    host.shutdown().await.expect("shutdown production Host");
}

async fn build_production_host(directory: &TempDir) -> ApplicationHost {
    ApplicationHostBuilder::new(
        directory.path(),
        HostPlatformServices::new(Arc::new(TestSecrets), Arc::new(NoopDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .with_builtin_protocol_package(Arc::from(crate::BUILTIN_ISO8583_COMPONENT))
    .build()
    .await
    .expect("build production environment services")
}

fn full_resource_candidate() -> Value {
    let mut value: Value = serde_json::from_slice(include_bytes!(
        "../../fixtures/environment_configuration_candidate_v1/full-shape.json"
    ))
    .expect("authoritative full-shape candidate");
    value["target"] = json!({"mode":"new","name":"Production Full Resources"});
    value["workspace"]["listeners"] = json!([
        value["workspace"]["listeners"][0].clone(),
        value["workspace"]["listeners"][1].clone(),
    ]);
    let http = &mut value["workspace"]["listeners"][0];
    http["enabled"] = json!(false);
    http["bind_address"] = json!("127.0.0.1");
    http["data_plane"]["settings"]["authentication"] = json!({"mode":"none"});
    http["data_plane"]["settings"]["mitm"] = json!({
        "enabled":false,
        "authority_allowlist":[],
        "root_ca_selector":null,
        "maximum_cached_leaf_certificates":256,
    });
    http["data_plane"]["settings"]["downstream_tls"] = json!({
        "enabled":false,
        "server_identity_alias":null,
        "dynamic_sni_allowlist":[],
        "client_authentication":{"mode":"disabled"},
    });
    http["data_plane"]["settings"]["body_processing"] = json!({
        "mode":"protocol",
        "package":{"id":"iso8583-ascii-standard","version":"1.0.0"},
    });
    http["data_plane"]["settings"]["fixed_server"] = Value::Null;
    let socket = &mut value["workspace"]["listeners"][1];
    socket["enabled"] = json!(false);
    socket["data_plane"]["settings"]["topology"] = json!({
        "mode":"local_responder",
        "settings":{"downstream_security":{"mode":"tcp"}},
    });
    socket["data_plane"]["settings"]["processing"] = json!({
        "mode":"scripted",
        "settings":{"package":{"id":"iso8583-ascii-standard","version":"1.0.0"}},
    });
    let document_rule = value["workspace"]["rules"]
        .as_array_mut()
        .expect("full-shape rules array")
        .iter_mut()
        .find(|rule| rule["name"] == "Protocol Document values")
        .expect("full-shape protocol Document rule");
    document_rule["content"]["value"]["package"] =
        json!({"id":"iso8583-ascii-standard","version":"1.0.0"});
    value["materials"] = json!({"certificates":[],"secrets":[]});
    value
}

async fn call(server: &McpServer, id: usize, name: &str, arguments: Value) -> Value {
    let response = post(
        server.local_addr(),
        "tools/call",
        Some(name),
        tool_call(id, name, &arguments),
    )
    .await;
    assert_eq!(response["result"]["isError"], false, "{response}");
    response["result"]["structuredContent"].clone()
}
