use super::*;

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
