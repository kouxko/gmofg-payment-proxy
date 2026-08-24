use super::*;
use crate::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};

fn scripted_processing() -> SocketPayloadProcessing {
    SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
        package: ProtocolPackageRef {
            id: ProtocolPackageId::new("iso8583-standard").unwrap(),
            version: ProtocolPackageVersion::new("1.2.3").unwrap(),
        },
    })
}

fn relay_topology(upstream: SocketEndpoint, security: SocketRelaySecurity) -> SocketTopology {
    SocketTopology::Relay(SocketRelayTopology { upstream, security })
}

fn relay_mut(settings: &mut SocketRelaySettings) -> &mut SocketRelayTopology {
    let SocketTopology::Relay(relay) = &mut settings.topology else {
        panic!("test fixture must remain Relay")
    };
    relay
}

#[test]
fn socket_target_and_capacity_are_validated_without_http_state() {
    let mut workspace = ProxyWorkspace::default();
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology: relay_topology(
            SocketEndpoint {
                host: "socket.example.test".into(),
                port: 16_127,
            },
            SocketRelaySecurity::Transparent,
        ),
        maximum_connections: DEFAULT_SOCKET_MAXIMUM_CONNECTIONS,
        runtime_limits: SocketRuntimeLimits::default(),
        processing: SocketPayloadProcessing::Direct,
    });
    workspace.validate().expect("valid socket relay");

    for host in [
        "",
        " socket.example.test",
        "https://socket.example.test",
        "user@socket.example.test",
        "socket.example.test/path",
        "socket.example.test?query=1",
    ] {
        let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
            unreachable!()
        };
        relay_mut(settings).upstream.host = host.into();
        assert!(workspace.validate().is_err(), "{host}");
    }

    if let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane {
        relay_mut(settings).upstream.host = "127.0.0.1".into();
        settings.maximum_connections = 0;
    }
    assert!(workspace.validate().is_err());
    if let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane {
        settings.maximum_connections = MAX_SOCKET_MAXIMUM_CONNECTIONS + 1;
    }
    assert!(workspace.validate().is_err());
}

#[test]
fn socket_runtime_limits_reject_zero_without_normalization() {
    let mut workspace = ProxyWorkspace::default();
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings::default());
    for mutate in [
        |limits: &mut SocketRuntimeLimits| limits.read_chunk_bytes = 0,
        |limits: &mut SocketRuntimeLimits| limits.diagnostic_event_capacity = 0,
        |limits: &mut SocketRuntimeLimits| limits.diagnostic_memory_bytes = 0,
    ] {
        let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
            unreachable!()
        };
        settings.runtime_limits = SocketRuntimeLimits::default();
        mutate(&mut settings.runtime_limits);
        assert!(workspace.validate().is_err());
    }
}

#[test]
fn socket_tls_roles_are_exhaustive_and_round_trip() {
    let server_identity = CertificateReferenceId::new();
    let client_trust = CertificateReferenceId::new();
    let server_trust = CertificateReferenceId::new();
    let client_identity = CertificateReferenceId::new();
    let references = [
        (
            server_identity,
            CertificateReferenceKind::ReverseServerIdentity,
        ),
        (
            client_trust,
            CertificateReferenceKind::DownstreamClientTrust,
        ),
        (server_trust, CertificateReferenceKind::UpstreamServerTrust),
        (
            client_identity,
            CertificateReferenceKind::UpstreamClientIdentity,
        ),
    ];
    let mut workspace = ProxyWorkspace {
        certificate_references: references
            .into_iter()
            .map(|(id, kind)| CertificateReference {
                id,
                label: format!("{kind:?}"),
                kind,
                reference: format!("managed:listener-tls:{id}"),
            })
            .collect(),
        ..ProxyWorkspace::default()
    };
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology: relay_topology(
            SocketEndpoint {
                host: "socket.example.test".into(),
                port: 443,
            },
            SocketRelaySecurity::TlsToTls {
                downstream_tls: SocketDownstreamTlsSettings {
                    server_identity,
                    client_authentication: DownstreamClientAuthentication::Required {
                        trust: client_trust,
                    },
                },
                upstream_tls: SocketUpstreamTlsSettings {
                    verify_hostname: true,
                    tls_server_name: None,
                    server_trust: Some(server_trust),
                    client_identity: Some(client_identity),
                },
            },
        ),
        maximum_connections: 500,
        runtime_limits: SocketRuntimeLimits::default(),
        processing: SocketPayloadProcessing::Direct,
    });

    workspace.validate().expect("all certificate roles match");
    let json = serde_json::to_vec(&workspace).unwrap();
    assert_eq!(
        serde_json::from_slice::<ProxyWorkspace>(&json).unwrap(),
        workspace
    );
    assert!(
        String::from_utf8(json)
            .unwrap()
            .contains("\"kind\":\"socket\"")
    );

    workspace.certificate_references[0].kind = CertificateReferenceKind::UpstreamServerTrust;
    assert!(workspace.validate().is_err());
}

#[test]
fn current_listener_rejects_unknown_fields() {
    let mut current = serde_json::to_value(ProxyListener::default()).unwrap();
    current["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ProxyListener>(current).is_err());

    for path in [
        &["data_plane", "settings", "authentication"][..],
        &[
            "data_plane",
            "settings",
            "downstream_tls",
            "client_authentication",
        ][..],
    ] {
        let mut current = serde_json::to_value(ProxyListener::default()).unwrap();
        let mut nested = &mut current;
        for segment in path {
            nested = &mut nested[*segment];
        }
        nested["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ProxyListener>(current).is_err());
    }

    let socket = ProxyListener {
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings::default()),
        ..ProxyListener::default()
    };
    let mut socket = serde_json::to_value(socket).unwrap();
    socket["data_plane"]["settings"]["security"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ProxyListener>(socket).is_err());
}

#[test]
fn scripted_processing_round_trips_only_the_exact_package_binding() {
    let processing = scripted_processing();
    let json = serde_json::to_value(&processing).unwrap();
    assert_eq!(json["settings"]["package"]["id"], "iso8583-standard");
    assert_eq!(json["settings"]["package"]["version"], "1.2.3");
    assert!(json["settings"].get("upstream").is_none());
    assert!(json["settings"].get("downstream").is_none());
    assert_eq!(
        serde_json::from_value::<SocketPayloadProcessing>(json).unwrap(),
        processing
    );
}

#[test]
fn socket_processing_default_is_direct_and_current_wire_is_strict() {
    assert_eq!(
        SocketPayloadProcessing::default(),
        SocketPayloadProcessing::Direct
    );
    assert_eq!(
        SocketRelaySettings::default().processing,
        SocketPayloadProcessing::Direct
    );

    let legacy = serde_json::json!({
        "upstream": { "host": "legacy.example.test", "port": 8123 },
        "security": { "mode": "transparent" },
        "maximum_connections": 12
    });
    assert!(serde_json::from_value::<SocketRelaySettings>(legacy).is_err());

    let missing_processing = serde_json::json!({
        "topology": {
            "mode": "relay",
            "settings": {
                "upstream": { "host": "server.example.test", "port": 8123 },
                "security": { "mode": "transparent" }
            }
        },
        "maximum_connections": 12
    });
    assert!(serde_json::from_value::<SocketRelaySettings>(missing_processing).is_err());
}

#[test]
fn socket_processing_rejects_incomplete_invalid_or_ambiguous_wire_data() {
    let scripted = serde_json::to_value(scripted_processing()).unwrap();

    let mut invalid_cases = Vec::new();

    let mut direct_with_script_settings = serde_json::json!({"mode": "direct"});
    direct_with_script_settings["settings"] = scripted["settings"].clone();
    invalid_cases.push(("direct with scripted settings", direct_with_script_settings));

    let mut missing_package = scripted.clone();
    missing_package["settings"]
        .as_object_mut()
        .unwrap()
        .remove("package");
    invalid_cases.push(("scripted without package", missing_package));

    let mut empty_id = scripted.clone();
    empty_id["settings"]["package"]["id"] = serde_json::json!("");
    invalid_cases.push(("empty package id", empty_id));

    let mut empty_version = scripted.clone();
    empty_version["settings"]["package"]["version"] = serde_json::json!("");
    invalid_cases.push(("empty package version", empty_version));

    let mut unknown_processing = scripted.clone();
    unknown_processing["unexpected"] = serde_json::json!(true);
    invalid_cases.push(("unknown processing field", unknown_processing));

    let mut unknown_scripted = scripted.clone();
    unknown_scripted["settings"]["unexpected"] = serde_json::json!(true);
    invalid_cases.push(("unknown scripted field", unknown_scripted));

    let mut obsolete_direction_switches = scripted;
    obsolete_direction_switches["settings"]["upstream"] = serde_json::json!({
        "decode_enabled": true,
        "encode_enabled": true
    });
    invalid_cases.push(("obsolete direction switches", obsolete_direction_switches));

    for (case, value) in invalid_cases {
        assert!(
            serde_json::from_value::<SocketPayloadProcessing>(value).is_err(),
            "{case} must fail closed"
        );
    }
}

#[test]
fn cloned_socket_workspace_edits_only_the_selected_package_binding() {
    let mut workspace = ProxyWorkspace::default();
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology: relay_topology(
            SocketEndpoint {
                host: "socket.example.test".into(),
                port: 16_127,
            },
            SocketRelaySecurity::Transparent,
        ),
        maximum_connections: DEFAULT_SOCKET_MAXIMUM_CONNECTIONS,
        runtime_limits: SocketRuntimeLimits::default(),
        processing: scripted_processing(),
    });
    let original = workspace.clone();

    let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    let SocketPayloadProcessing::Scripted(scripted) = &mut settings.processing else {
        unreachable!()
    };
    scripted.package.version = ProtocolPackageVersion::new("2.0.0").unwrap();

    let ListenerDataPlane::Socket(original_settings) = &original.listeners[0].data_plane else {
        unreachable!()
    };
    let SocketPayloadProcessing::Scripted(original_scripted) = &original_settings.processing else {
        unreachable!()
    };
    assert_eq!(original_scripted.package.version.as_str(), "1.2.3");

    let SocketPayloadProcessing::Scripted(edited) = &settings.processing else {
        unreachable!()
    };
    assert_eq!(edited.package.version.as_str(), "2.0.0");
    original.validate().unwrap();
    workspace.validate().unwrap();
}
