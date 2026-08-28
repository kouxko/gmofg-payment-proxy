use super::*;
use crate::{
    ListenerDataPlane, MAX_JAVASCRIPT_SAFE_INTEGER, ProtocolRuleStage, ProxyListener,
    ProxyWorkspace, RuleContent, RuleDefinition, RuleDefinitionDraft, RuleId, RuleStage,
    ScriptedSocketProcessing, SocketDownstreamSecurity, SocketLocalResponderTopology,
    SocketPayloadProcessing, SocketRelaySecurity, SocketRelaySettings, SocketRuleContent,
    SocketRuntimeLimits, SocketTopology,
};

fn unified(rule: &ProtocolDocumentRuleDefinition) -> RuleDefinition {
    RuleDefinition::restore(
        RuleId::from_uuid(rule.rule_id().as_uuid()),
        rule.revision(),
        RuleDefinitionDraft {
            name: rule.name().to_owned(),
            enabled: rule.enabled(),
            priority: rule.priority(),
            listener_id: rule.listener_id(),
            stage: match rule.stage() {
                ProtocolRuleStage::AppToProxy => RuleStage::AppToProxy,
                ProtocolRuleStage::ProxyToUpstream => RuleStage::ProxyToUpstream,
                ProtocolRuleStage::UpstreamToProxy => RuleStage::UpstreamToProxy,
                ProtocolRuleStage::ProxyToApp => RuleStage::ProxyToApp,
            },
            content: RuleContent::Socket(SocketRuleContent {
                package: rule.package().clone(),
                schema_version: rule.schema_version(),
                conditions: rule.conditions().to_vec(),
                actions: rule.actions().to_vec(),
            }),
        },
        rule.created_order(),
    )
    .unwrap()
}

fn scripted_listener(topology: SocketTopology) -> ProxyListener {
    ProxyListener {
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
            topology,
            maximum_connections: 32,
            runtime_limits: SocketRuntimeLimits::default(),
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: package("1.2.3"),
            }),
        }),
        ..ProxyListener::default()
    }
}

fn relay_listener() -> ProxyListener {
    scripted_listener(SocketTopology::Relay(crate::SocketRelayTopology {
        upstream: crate::SocketEndpoint {
            host: "server.test".into(),
            port: 9000,
        },
        security: SocketRelaySecurity::Transparent,
    }))
}

fn local_listener() -> ProxyListener {
    scripted_listener(SocketTopology::LocalResponder(
        SocketLocalResponderTopology {
            downstream_security: SocketDownstreamSecurity::Tcp,
        },
    ))
}

#[test]
fn local_response_accepts_only_the_two_application_boundary_stages() {
    let listener = local_listener();
    for (index, stage) in [ProtocolRuleStage::AppToProxy, ProtocolRuleStage::ProxyToApp]
        .into_iter()
        .enumerate()
    {
        let mut workspace = ProxyWorkspace {
            listeners: vec![listener.clone()],
            rule_created_order_high_water: index as u64 + 1,
            ..ProxyWorkspace::default()
        };
        workspace.rule_definitions.push(unified(
            &ProtocolDocumentRuleDefinition::create(
                ProtocolDocumentRuleDraft {
                    name: format!("allowed-{stage:?}"),
                    enabled: true,
                    priority: 1,
                    listener_id: listener.id,
                    package: package("1.2.3"),
                    schema_version: 1,
                    stage,
                    conditions: Vec::new(),
                    actions: vec![DocumentAction::RecordMatch],
                },
                index as u64 + 1,
            )
            .unwrap(),
        ));
        workspace.validate().unwrap();
    }

    for (index, stage) in [
        ProtocolRuleStage::ProxyToUpstream,
        ProtocolRuleStage::UpstreamToProxy,
    ]
    .into_iter()
    .enumerate()
    {
        let mut workspace = ProxyWorkspace {
            listeners: vec![listener.clone()],
            rule_created_order_high_water: index as u64 + 1,
            ..ProxyWorkspace::default()
        };
        workspace.rule_definitions.push(unified(
            &ProtocolDocumentRuleDefinition::create(
                ProtocolDocumentRuleDraft {
                    name: format!("rejected-{stage:?}"),
                    enabled: true,
                    priority: 1,
                    listener_id: listener.id,
                    package: package("1.2.3"),
                    schema_version: 1,
                    stage,
                    conditions: Vec::new(),
                    actions: vec![DocumentAction::RecordMatch],
                },
                index as u64 + 1,
            )
            .unwrap(),
        ));
        let error = workspace.validate().unwrap_err();
        assert!(error.field_errors.contains_key("rule_definitions.0.stage"));
    }
}

#[test]
fn rejects_cross_listener_package_and_http_references() {
    let socket = relay_listener();
    let mut workspace = ProxyWorkspace {
        listeners: vec![socket.clone()],
        rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    workspace.rule_definitions.push(unified(
        &rule(
            1,
            socket.id,
            ProtocolDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
    ));
    workspace.validate().unwrap();

    let mut missing = workspace.clone();
    missing.rule_definitions = vec![unified(
        &rule(
            2,
            ListenerId::new(),
            ProtocolDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
    )];
    assert!(missing.validate().is_err());

    let mut wrong_package = workspace.clone();
    let mut json = serde_json::to_value(&wrong_package.rule_definitions[0]).unwrap();
    json["content"]["value"]["package"]["version"] = serde_json::json!("9.9.9");
    wrong_package.rule_definitions[0] = serde_json::from_value(json).unwrap();
    assert!(wrong_package.validate().is_err());

    let http_id = ProxyWorkspace::default().listeners[0].id;
    let mut http = ProxyWorkspace::default();
    http.rule_definitions.push(unified(
        &rule(
            3,
            http_id,
            ProtocolDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
    ));
    assert!(http.validate().is_err());
}

#[test]
fn workspace_high_water_validates_monotonic_boundaries() {
    let listener = relay_listener();
    let stored_rule = rule(
        7,
        listener.id,
        ProtocolDirection::Upstream,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener],
        rule_definitions: vec![unified(&stored_rule)],
        rule_created_order_high_water: 6,
        ..ProxyWorkspace::default()
    };
    assert!(
        workspace
            .validate()
            .unwrap_err()
            .field_errors
            .contains_key("rule_created_order_high_water")
    );

    workspace.rule_created_order_high_water = 7;
    workspace.validate().unwrap();
    workspace.rule_created_order_high_water = MAX_JAVASCRIPT_SAFE_INTEGER + 1;
    assert!(
        workspace
            .validate()
            .unwrap_err()
            .field_errors
            .contains_key("rule_created_order_high_water")
    );
}
