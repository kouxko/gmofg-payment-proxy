use super::*;

#[test]
fn socket_target_and_capacity_are_validated_without_http_state() {
    let mut workspace = ProxyWorkspace::default();
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        upstream: SocketEndpoint {
            host: "socket.example.test".into(),
            port: 16_127,
        },
        security: SocketRelaySecurity::Transparent,
        maximum_connections: DEFAULT_SOCKET_MAXIMUM_CONNECTIONS,
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
        settings.upstream.host = host.into();
        assert!(workspace.validate().is_err(), "{host}");
    }

    if let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane {
        settings.upstream.host = "127.0.0.1".into();
        settings.maximum_connections = 0;
    }
    assert!(workspace.validate().is_err());
    if let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane {
        settings.maximum_connections = MAX_SOCKET_MAXIMUM_CONNECTIONS + 1;
    }
    assert!(workspace.validate().is_err());
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
        upstream: SocketEndpoint {
            host: "socket.example.test".into(),
            port: 443,
        },
        security: SocketRelaySecurity::TlsToTls {
            downstream_tls: SocketDownstreamTlsSettings {
                server_identity,
                client_authentication: DownstreamClientAuthentication::Required {
                    trust: client_trust,
                },
            },
            upstream_tls: SocketUpstreamTlsSettings {
                verify_hostname: true,
                server_trust: Some(server_trust),
                client_identity: Some(client_identity),
            },
        },
        maximum_connections: 500,
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
fn current_listener_rejects_legacy_flat_or_unknown_fields() {
    let legacy = serde_json::to_value(v2_listener()).unwrap();
    assert!(serde_json::from_value::<ProxyListener>(legacy).is_err());

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
