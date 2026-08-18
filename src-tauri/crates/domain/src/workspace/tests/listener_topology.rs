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

fn local_responder(downstream_security: SocketDownstreamSecurity) -> SocketRelaySettings {
    SocketRelaySettings {
        topology: SocketTopology::LocalResponder(SocketLocalResponderTopology {
            downstream_security,
        }),
        maximum_connections: 32,
        processing: scripted_processing(),
    }
}

#[test]
fn local_responder_tcp_and_tls_round_trip_without_processing_switches() {
    let server_identity = CertificateReferenceId::new();
    let client_trust = CertificateReferenceId::new();
    let security_modes = [
        SocketDownstreamSecurity::Tcp,
        SocketDownstreamSecurity::Tls {
            downstream_tls: SocketDownstreamTlsSettings {
                server_identity,
                client_authentication: DownstreamClientAuthentication::Required {
                    trust: client_trust,
                },
            },
        },
    ];
    for downstream_security in security_modes {
        let settings = local_responder(downstream_security);
        let json = serde_json::to_value(&settings).unwrap();
        assert!(json["topology"]["settings"].get("upstream").is_none());
        assert!(json["topology"]["settings"].get("security").is_none());
        assert!(json["processing"]["settings"].get("upstream").is_none());
        assert!(json["processing"]["settings"].get("downstream").is_none());
        assert_eq!(
            serde_json::from_value::<SocketRelaySettings>(json).unwrap(),
            settings
        );
    }
}

#[test]
fn local_responder_tls_references_are_validated_at_exact_wire_paths() {
    let server_identity = CertificateReferenceId::new();
    let client_trust = CertificateReferenceId::new();
    let mut workspace = ProxyWorkspace {
        certificate_references: vec![
            CertificateReference {
                id: server_identity,
                label: "server identity".into(),
                kind: CertificateReferenceKind::ReverseServerIdentity,
                reference: format!("managed:listener-tls:{server_identity}"),
            },
            CertificateReference {
                id: client_trust,
                label: "client trust".into(),
                kind: CertificateReferenceKind::DownstreamClientTrust,
                reference: format!("managed:listener-tls:{client_trust}"),
            },
        ],
        ..ProxyWorkspace::default()
    };
    workspace.listeners[0].data_plane =
        ListenerDataPlane::Socket(local_responder(SocketDownstreamSecurity::Tls {
            downstream_tls: SocketDownstreamTlsSettings {
                server_identity,
                client_authentication: DownstreamClientAuthentication::Required {
                    trust: client_trust,
                },
            },
        }));
    workspace.validate().expect("valid App-side TLS references");

    workspace.certificate_references[0].kind = CertificateReferenceKind::UpstreamServerTrust;
    let error = workspace.validate().expect_err("wrong certificate kind");
    assert!(error.field_errors.contains_key(
        "listeners.0.data_plane.settings.topology.settings.downstream_security.downstream_tls.server_identity"
    ));
    workspace.certificate_references.clear();
    let error = workspace
        .validate()
        .expect_err("unknown certificate references");
    assert!(error.field_errors.contains_key(
        "listeners.0.data_plane.settings.topology.settings.downstream_security.downstream_tls.client_authentication"
    ));
}

#[test]
fn local_responder_requires_protocol_processing() {
    let valid = local_responder(SocketDownstreamSecurity::Tcp);
    let mut workspace = ProxyWorkspace::default();
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(valid.clone());
    workspace.validate().expect("valid LocalResponder");

    if let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane {
        settings.processing = SocketPayloadProcessing::Direct;
    }
    assert!(workspace.validate().is_err());
}

#[test]
fn topology_wire_rejects_unknown_or_cross_mode_fields() {
    let local = serde_json::to_value(local_responder(SocketDownstreamSecurity::Tcp)).unwrap();
    let mut cases = Vec::new();
    let mut unknown_tag = local.clone();
    unknown_tag["topology"]["mode"] = serde_json::json!("automatic");
    cases.push(unknown_tag);
    let mut forged_upstream = local.clone();
    forged_upstream["topology"]["settings"]["upstream"] =
        serde_json::json!({ "host": "server.example.test", "port": 443 });
    cases.push(forged_upstream);
    let mut forged_tls = local.clone();
    forged_tls["topology"]["settings"]["upstream_tls"] =
        serde_json::json!({ "verify_hostname": true });
    cases.push(forged_tls);
    let mut mixed_legacy = local;
    mixed_legacy["upstream"] = serde_json::json!({ "host": "server.example.test", "port": 443 });
    cases.push(mixed_legacy);

    for value in cases {
        assert!(serde_json::from_value::<SocketRelaySettings>(value).is_err());
    }
}
