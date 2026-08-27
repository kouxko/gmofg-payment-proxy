use super::*;

#[tokio::test]
async fn editor_context_owns_relay_stages_capabilities_and_new_rule_drafts() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;

    let context = application
        .protocol_rule_editor_context(listener_id)
        .await
        .unwrap();

    assert_eq!(context.listener_id, listener_id);
    assert_eq!(context.package, package);
    assert_eq!(
        context
            .stages
            .iter()
            .map(|item| item.stage)
            .collect::<Vec<_>>(),
        [
            ProtocolRuleStage::AppToProxy,
            ProtocolRuleStage::ProxyToUpstream,
            ProtocolRuleStage::UpstreamToProxy,
            ProtocolRuleStage::ProxyToApp,
        ]
    );
    assert_eq!(
        context
            .stages
            .iter()
            .map(|item| item.schema_version)
            .collect::<Vec<_>>(),
        [1, 1, 2, 2]
    );
    for stage in &context.stages {
        assert_eq!(stage.new_rule_draft.rule_id, None);
        assert_eq!(stage.new_rule_draft.expected_revision, None);
        assert_eq!(stage.new_rule_draft.name, "新规则");
        assert!(stage.new_rule_draft.enabled);
        assert_eq!(stage.new_rule_draft.priority, 100);
        assert_eq!(stage.new_rule_draft.listener_id, listener_id);
        assert_eq!(stage.new_rule_draft.package, context.package);
        assert_eq!(stage.new_rule_draft.schema_version, stage.schema_version);
        assert_eq!(stage.new_rule_draft.stage, stage.stage);
        assert!(stage.new_rule_draft.conditions.is_empty());
        assert_eq!(stage.new_rule_draft.actions, [DocumentAction::RecordMatch]);
    }

    let mut json = serde_json::to_value(&context).unwrap();
    assert!(json.get("listener_id").is_some());
    assert!(json["stages"][0].get("new_rule_draft").is_some());
    assert!(json.get("listenerId").is_none());
    json["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ProtocolRuleEditorContext>(json).is_err());
}

#[tokio::test]
async fn editor_context_owns_local_responder_stages_and_clear_document_default() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    settings.topology = SocketTopology::LocalResponder(SocketLocalResponderTopology::default());
    workspaces.save(workspace).await.unwrap();

    let context = application
        .protocol_rule_editor_context(listener_id)
        .await
        .unwrap();

    assert_eq!(
        context
            .stages
            .iter()
            .map(|item| item.stage)
            .collect::<Vec<_>>(),
        [ProtocolRuleStage::AppToProxy, ProtocolRuleStage::ProxyToApp,]
    );
    assert!(
        context
            .stages
            .iter()
            .all(|stage| { stage.new_rule_draft.actions == [DocumentAction::ClearDocument] })
    );
}

#[tokio::test]
async fn editor_context_rejects_entries_without_a_protocol_package() {
    let (application, _, workspaces, _) = fixture();
    let selected = workspaces.list().await.unwrap().remove(0);
    let workspace = workspaces.get(selected.id).await.unwrap();
    let listener_id = workspace.listeners[0].id;

    let error = application
        .protocol_rule_editor_context(listener_id)
        .await
        .unwrap_err();

    assert_eq!(error_code(&error), "DOCUMENT_RULE_PROTOCOL_REQUIRED");
}

#[tokio::test]
async fn editor_context_owns_all_http_protocol_stages_and_defaults() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("http-json", "1.0.0");
    let listener_id = configure_http(&services, &workspaces, &package).await;

    let context = application
        .protocol_rule_editor_context(listener_id)
        .await
        .unwrap();

    assert_eq!(context.stages.len(), 4);
    assert_eq!(
        context
            .stages
            .iter()
            .map(|item| item.stage)
            .collect::<Vec<_>>(),
        [
            ProtocolRuleStage::AppToProxy,
            ProtocolRuleStage::ProxyToUpstream,
            ProtocolRuleStage::UpstreamToProxy,
            ProtocolRuleStage::ProxyToApp,
        ]
    );
    assert!(context.stages.iter().all(|stage| {
        stage.new_rule_draft.listener_id == listener_id
            && stage.new_rule_draft.package == package
            && stage.new_rule_draft.stage == stage.stage
            && stage.new_rule_draft.schema_version == stage.schema_version
            && stage.new_rule_draft.actions == [DocumentAction::RecordMatch]
    }));
}

#[tokio::test]
async fn editor_context_rejects_direct_socket_and_unknown_listener() {
    let (application, _, workspaces, _) = fixture();
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology: SocketTopology::Relay(SocketRelayTopology {
            upstream: SocketEndpoint {
                host: "127.0.0.1".into(),
                port: 9000,
            },
            security: SocketRelaySecurity::Transparent,
        }),
        maximum_connections: 8,
        runtime_limits: intercept_proxy_domain::SocketRuntimeLimits::default(),
        processing: SocketPayloadProcessing::Direct,
    });
    let listener_id = workspace.listeners[0].id;
    workspaces.save(workspace).await.unwrap();

    let direct_error = application
        .protocol_rule_editor_context(listener_id)
        .await
        .unwrap_err();
    assert_eq!(error_code(&direct_error), "DOCUMENT_RULE_PROTOCOL_REQUIRED");

    let missing_error = application
        .protocol_rule_editor_context(ListenerId::new())
        .await
        .unwrap_err();
    assert_eq!(
        error_code(&missing_error),
        "PROTOCOL_RULE_LISTENER_NOT_FOUND"
    );
}

#[tokio::test]
async fn relay_exposes_all_stages_and_local_responder_limits_stages_only_by_topology() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    for (stage, expected_schema_version) in [
        (ProtocolRuleStage::AppToProxy, 1),
        (ProtocolRuleStage::ProxyToUpstream, 1),
        (ProtocolRuleStage::UpstreamToProxy, 2),
        (ProtocolRuleStage::ProxyToApp, 2),
    ] {
        let capabilities = application
            .protocol_rule_capabilities(listener_id, stage)
            .await
            .unwrap();
        assert_eq!(capabilities.schema_version, expected_schema_version);

        let mut wrong_schema = input(listener_id, package.clone(), stage.direction(), 0);
        wrong_schema.stage = stage;
        wrong_schema.schema_version = if expected_schema_version == 1 { 2 } else { 1 };
        assert_eq!(
            error_code(
                &application
                    .protocol_rule_save(wrong_schema)
                    .await
                    .unwrap_err()
            ),
            "PROTOCOL_RULE_SCHEMA_MISMATCH"
        );
    }

    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    settings.topology = SocketTopology::LocalResponder(SocketLocalResponderTopology::default());
    workspaces.save(workspace).await.unwrap();
    let mut static_response = input(
        listener_id,
        package.clone(),
        ProtocolDirection::Downstream,
        0,
    );
    static_response.conditions = vec![equals("trace_id", DocumentValue::String("x".into()))];
    static_response.actions = vec![set("trace_id", DocumentValue::String("00".into()))];
    application
        .protocol_rule_save(static_response)
        .await
        .unwrap();
    assert_eq!(
        error_code(
            &application
                .protocol_rule_capabilities(listener_id, ProtocolRuleStage::ProxyToUpstream)
                .await
                .unwrap_err()
        ),
        "PROTOCOL_RULE_DIRECTION_INVALID"
    );
}
