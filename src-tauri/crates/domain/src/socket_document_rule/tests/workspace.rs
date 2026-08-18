use super::*;
use crate::{
    DirectionProcessingOptions, ListenerDataPlane, MAX_JAVASCRIPT_SAFE_INTEGER, ProxyListener,
    ProxyWorkspace, ScriptedSocketProcessing, SocketDownstreamSecurity,
    SocketLocalResponderTopology, SocketPayloadProcessing, SocketRelaySecurity,
    SocketRelaySettings, SocketTopology,
};

fn scripted_listener(topology: SocketTopology) -> ProxyListener {
    ProxyListener {
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
            topology,
            maximum_connections: 32,
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: package("1.2.3"),
                upstream: DirectionProcessingOptions {
                    decode_enabled: true,
                    encode_enabled: true,
                },
                downstream: DirectionProcessingOptions {
                    decode_enabled: true,
                    encode_enabled: true,
                },
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

#[test]
fn rejects_cross_listener_package_and_http_references() {
    let socket = relay_listener();
    let mut workspace = ProxyWorkspace {
        listeners: vec![socket.clone()],
        socket_rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    workspace.socket_rules.push(
        rule(
            1,
            socket.id,
            SocketDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
    );
    workspace.validate().unwrap();

    let mut missing = workspace.clone();
    missing.socket_rules = vec![
        rule(
            2,
            ListenerId::new(),
            SocketDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
    ];
    assert!(missing.validate().is_err());

    let mut wrong_package = workspace.clone();
    let mut json = serde_json::to_value(&wrong_package.socket_rules[0]).unwrap();
    json["package"]["version"] = serde_json::json!("9.9.9");
    wrong_package.socket_rules[0] = serde_json::from_value(json).unwrap();
    assert!(wrong_package.validate().is_err());

    let http_id = ProxyWorkspace::default().listeners[0].id;
    let mut http = ProxyWorkspace::default();
    http.socket_rules.push(
        rule(
            3,
            http_id,
            SocketDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
    );
    assert!(http.validate().is_err());
}

#[test]
fn relay_enforces_direction_processing_options() {
    let mut listener = relay_listener();
    {
        let ListenerDataPlane::Socket(settings) = &mut listener.data_plane else {
            unreachable!()
        };
        let SocketPayloadProcessing::Scripted(scripted) = &mut settings.processing else {
            unreachable!()
        };
        scripted.upstream.decode_enabled = false;
        scripted.upstream.encode_enabled = false;
    }
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        socket_rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    workspace.socket_rules.push(
        rule(
            1,
            listener.id,
            SocketDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
    );
    assert!(
        workspace
            .validate()
            .unwrap_err()
            .field_errors
            .contains_key("socket_rules.0.direction")
    );

    let ListenerDataPlane::Socket(settings) = &mut listener.data_plane else {
        unreachable!()
    };
    let SocketPayloadProcessing::Scripted(scripted) = &mut settings.processing else {
        unreachable!()
    };
    scripted.upstream.decode_enabled = true;
    let mut modified = ProxyWorkspace {
        listeners: vec![listener.clone()],
        socket_rule_created_order_high_water: 2,
        ..ProxyWorkspace::default()
    };
    modified.socket_rules.push(
        rule(
            2,
            listener.id,
            SocketDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::ClearDocument],
        )
        .unwrap(),
    );
    assert!(
        modified
            .validate()
            .unwrap_err()
            .field_errors
            .contains_key("socket_rules.0.actions")
    );
}

#[test]
fn local_responder_allows_static_response_but_rejects_direction_and_encode_mismatch() {
    let mut listener = scripted_listener(SocketTopology::LocalResponder(
        SocketLocalResponderTopology {
            downstream_security: SocketDownstreamSecurity::Tcp,
        },
    ));
    let ListenerDataPlane::Socket(settings) = &mut listener.data_plane else {
        unreachable!()
    };
    let SocketPayloadProcessing::Scripted(scripted) = &mut settings.processing else {
        unreachable!()
    };
    scripted.upstream.encode_enabled = false;
    scripted.downstream.decode_enabled = false;
    scripted.downstream.encode_enabled = false;

    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        socket_rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    workspace.socket_rules.push(
        rule(
            1,
            listener.id,
            SocketDirection::Downstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .unwrap(),
    );
    workspace.validate().unwrap();

    workspace.socket_rules[0] = rule(
        2,
        listener.id,
        SocketDirection::Upstream,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    assert!(
        workspace
            .validate()
            .unwrap_err()
            .field_errors
            .contains_key("socket_rules.0.direction")
    );
    workspace.socket_rules[0] = rule(
        3,
        listener.id,
        SocketDirection::Downstream,
        Vec::new(),
        vec![DocumentAction::ClearDocument],
    )
    .unwrap();
    assert!(
        workspace
            .validate()
            .unwrap_err()
            .field_errors
            .contains_key("socket_rules.0.actions")
    );
}

#[test]
fn workspace_high_water_defaults_for_legacy_json_and_validates_monotonic_boundaries() {
    let mut legacy = serde_json::to_value(ProxyWorkspace::default()).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("socket_rule_created_order_high_water");
    let restored: ProxyWorkspace = serde_json::from_value(legacy).unwrap();
    assert_eq!(restored.socket_rule_created_order_high_water, 0);
    restored.validate().unwrap();

    let listener = relay_listener();
    let stored_rule = rule(
        7,
        listener.id,
        SocketDirection::Upstream,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener],
        socket_rules: vec![stored_rule],
        socket_rule_created_order_high_water: 6,
        ..ProxyWorkspace::default()
    };
    assert!(
        workspace
            .validate()
            .unwrap_err()
            .field_errors
            .contains_key("socket_rule_created_order_high_water")
    );

    workspace.socket_rule_created_order_high_water = 7;
    workspace.validate().unwrap();
    workspace.socket_rule_created_order_high_water = MAX_JAVASCRIPT_SAFE_INTEGER + 1;
    assert!(
        workspace
            .validate()
            .unwrap_err()
            .field_errors
            .contains_key("socket_rule_created_order_high_water")
    );
}
