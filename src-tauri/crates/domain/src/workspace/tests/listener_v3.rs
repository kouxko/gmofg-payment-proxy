use super::*;
use crate::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};

fn scripted_processing(
    upstream: DirectionProcessingOptions,
    downstream: DirectionProcessingOptions,
) -> SocketPayloadProcessing {
    SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
        package: ProtocolPackageRef {
            id: ProtocolPackageId::new("iso8583-standard").unwrap(),
            version: ProtocolPackageVersion::new("1.2.3").unwrap(),
        },
        upstream,
        downstream,
    })
}

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

#[test]
fn socket_processing_all_sixteen_switch_combinations_round_trip_without_direction_swaps() {
    // 四个布尔值共同组成 16 种合法配置。逐位生成而不是只抽样，确保 Serde 字段映射不会把
    // Upstream/Downstream 或 Decode/Encode 接反。
    for mask in 0_u8..16 {
        let upstream_decode = mask & 0b0001 != 0;
        let upstream_encode = mask & 0b0010 != 0;
        let downstream_decode = mask & 0b0100 != 0;
        let downstream_encode = mask & 0b1000 != 0;
        let processing = scripted_processing(
            DirectionProcessingOptions {
                decode_enabled: upstream_decode,
                encode_enabled: upstream_encode,
            },
            DirectionProcessingOptions {
                decode_enabled: downstream_decode,
                encode_enabled: downstream_encode,
            },
        );

        let json = serde_json::to_value(&processing).unwrap();
        let settings = &json["settings"];
        assert_eq!(settings["upstream"]["decode_enabled"], upstream_decode);
        assert_eq!(settings["upstream"]["encode_enabled"], upstream_encode);
        assert_eq!(settings["downstream"]["decode_enabled"], downstream_decode);
        assert_eq!(settings["downstream"]["encode_enabled"], downstream_encode);
        assert_eq!(
            serde_json::from_value::<SocketPayloadProcessing>(json).unwrap(),
            processing,
            "four-switch mask {mask:04b} must round-trip exactly"
        );
    }
}

#[test]
fn socket_processing_defaults_and_historical_settings_are_direct() {
    assert_eq!(
        SocketPayloadProcessing::default(),
        SocketPayloadProcessing::Direct
    );
    assert_eq!(
        SocketRelaySettings::default().processing,
        SocketPayloadProcessing::Direct
    );

    // T04 之前保存的 Socket settings 只有 upstream/security/maximum_connections。
    // 删除新字段模拟真实历史 JSON，反序列化必须无损迁移为 Direct。
    let mut historical = serde_json::to_value(SocketRelaySettings::default()).unwrap();
    historical.as_object_mut().unwrap().remove("processing");
    let migrated: SocketRelaySettings = serde_json::from_value(historical).unwrap();
    assert_eq!(migrated.processing, SocketPayloadProcessing::Direct);
    assert_eq!(
        serde_json::to_value(migrated).unwrap()["processing"],
        serde_json::json!({"mode": "direct"})
    );
}

#[test]
fn socket_processing_rejects_incomplete_invalid_or_ambiguous_wire_data() {
    let scripted = serde_json::to_value(scripted_processing(
        DirectionProcessingOptions {
            decode_enabled: true,
            encode_enabled: false,
        },
        DirectionProcessingOptions {
            decode_enabled: false,
            encode_enabled: true,
        },
    ))
    .unwrap();

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

    let mut unknown_direction = scripted;
    unknown_direction["settings"]["upstream"]["unexpected"] = serde_json::json!(true);
    invalid_cases.push(("unknown direction field", unknown_direction));

    for (case, value) in invalid_cases {
        assert!(
            serde_json::from_value::<SocketPayloadProcessing>(value).is_err(),
            "{case} must fail closed"
        );
    }
}

#[test]
fn cloned_socket_workspace_edits_only_the_selected_direction() {
    let mut workspace = ProxyWorkspace::default();
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        upstream: SocketEndpoint {
            host: "socket.example.test".into(),
            port: 16_127,
        },
        security: SocketRelaySecurity::Transparent,
        maximum_connections: DEFAULT_SOCKET_MAXIMUM_CONNECTIONS,
        processing: scripted_processing(
            DirectionProcessingOptions {
                decode_enabled: true,
                encode_enabled: true,
            },
            DirectionProcessingOptions {
                decode_enabled: false,
                encode_enabled: true,
            },
        ),
    });
    let original = workspace.clone();

    let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    let SocketPayloadProcessing::Scripted(scripted) = &mut settings.processing else {
        unreachable!()
    };
    scripted.upstream.decode_enabled = false;

    let ListenerDataPlane::Socket(original_settings) = &original.listeners[0].data_plane else {
        unreachable!()
    };
    let SocketPayloadProcessing::Scripted(original_scripted) = &original_settings.processing else {
        unreachable!()
    };
    assert!(original_scripted.upstream.decode_enabled);
    assert!(original_scripted.upstream.encode_enabled);
    assert!(!original_scripted.downstream.decode_enabled);
    assert!(original_scripted.downstream.encode_enabled);

    let SocketPayloadProcessing::Scripted(edited) = &settings.processing else {
        unreachable!()
    };
    assert!(!edited.upstream.decode_enabled);
    assert!(edited.upstream.encode_enabled);
    assert_eq!(edited.downstream, original_scripted.downstream);
    original.validate().unwrap();
    workspace.validate().unwrap();
}
