use super::*;
use intercept_proxy_domain::{
    DownstreamClientAuthentication, HttpTopology, ListenerDataPlane, ScriptedSocketProcessing,
    SocketDownstreamTlsSettings, SocketEndpoint, SocketPayloadProcessing, SocketRelaySecurity,
    SocketRelaySettings, SocketRelayTopology, SocketRuntimeLimits, SocketTopology,
    SocketUpstreamTlsSettings,
};

mod validation;

fn recommended_package() -> ProtocolPackageRef {
    builtin_iso8583_package_ref()
}

fn full_socket_listener() -> ProxyListener {
    let downstream_tls = SocketDownstreamTlsSettings {
        server_identity: CertificateReferenceId::new(),
        client_authentication: DownstreamClientAuthentication::Required {
            trust: CertificateReferenceId::new(),
        },
    };
    ProxyListener {
        id: ListenerId::new(),
        name: "Socket payment".into(),
        enabled: true,
        bind_address: "0.0.0.0".into(),
        port: 18_001,
        connect_timeout_ms: 1_001,
        read_timeout_ms: 2_002,
        write_timeout_ms: 3_003,
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
            topology: SocketTopology::Relay(SocketRelayTopology {
                upstream: SocketEndpoint {
                    host: "payments.example.test".into(),
                    port: 18_002,
                },
                security: SocketRelaySecurity::TlsToTls {
                    downstream_tls,
                    upstream_tls: SocketUpstreamTlsSettings {
                        verify_hostname: false,
                        tls_server_name: Some("tls.example.test".into()),
                        server_trust: Some(CertificateReferenceId::new()),
                        client_identity: Some(CertificateReferenceId::new()),
                    },
                },
            }),
            maximum_connections: 64,
            runtime_limits: SocketRuntimeLimits::default(),
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: recommended_package(),
            }),
        }),
    }
}

#[test]
fn complete_listener_round_trips_through_application_draft_exactly() {
    for listener in [ProxyListener::default(), full_socket_listener()] {
        let draft = ListenerDraft::from(listener.clone());
        assert_eq!(draft.clone().try_into_listener(0).unwrap(), listener);
        assert_eq!(
            ListenerDraft::from(draft.clone().try_into_listener(0).unwrap()),
            draft
        );
    }
}

#[test]
fn draft_copy_changes_only_identity_name_and_enabled_state() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let source = ListenerDraft::from(full_socket_listener());
    let copy = application.listener_draft_copy(source.clone()).unwrap();

    assert_ne!(copy.id, source.id);
    assert_eq!(copy.name, "Socket payment 副本");
    assert!(!copy.enabled);
    assert_eq!(copy.data_plane, source.data_plane);
    assert_eq!(copy.bind_address, source.bind_address);
    assert_eq!(copy.port, source.port);
    assert_eq!(copy.connect_timeout_ms, source.connect_timeout_ms);
    assert_eq!(copy.read_timeout_ms, source.read_timeout_ms);
    assert_eq!(copy.write_timeout_ms, source.write_timeout_ms);
}

#[test]
fn data_plane_and_http_response_transitions_use_domain_defaults() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let original = ListenerDraft::from(ProxyListener::default());
    let socket = application
        .listener_draft_transition(
            original.clone(),
            ListenerDraftTransition::DataPlane {
                kind: ListenerDataPlaneKind::Socket,
            },
            None,
        )
        .unwrap();
    let expected_socket = ListenerDraft {
        data_plane: ListenerDataPlaneDraft::Socket(SocketRelayDraftSettings::default()),
        ..original.clone()
    };
    assert_eq!(socket, expected_socket);
    assert_eq!(
        application
            .listener_draft_transition(
                socket.clone(),
                ListenerDraftTransition::DataPlane {
                    kind: ListenerDataPlaneKind::Socket,
                },
                None,
            )
            .unwrap(),
        socket
    );

    let fixed = application
        .listener_draft_transition(
            original,
            ListenerDraftTransition::HttpResponseMode {
                mode: HttpResponseMode::FixedServer,
            },
            None,
        )
        .unwrap();
    let ListenerDataPlaneDraft::Http(settings) = &fixed.data_plane else {
        panic!("HTTP transition must retain HTTP data plane");
    };
    assert_eq!(
        settings.topology,
        HttpTopology::remote(Some(FixedServerSettings {
            upstream_url: String::new(),
            upstream_tls: UpstreamTlsSettings::default(),
        }))
    );
}

#[test]
fn transition_rejects_intents_for_the_wrong_data_plane() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let http = ListenerDraft::from(ProxyListener::default());
    let error = application
        .listener_draft_transition(
            http,
            ListenerDraftTransition::SocketTopology {
                mode: SocketTopologyMode::Relay,
            },
            None,
        )
        .expect_err("Socket intent cannot silently succeed for HTTP");
    assert_eq!(
        error.view_model.code,
        "LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE"
    );

    let socket = ListenerDraft::from(ProxyListener {
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings::default()),
        ..ProxyListener::default()
    });
    let error = application
        .listener_draft_transition(
            socket,
            ListenerDraftTransition::HttpResponseMode {
                mode: HttpResponseMode::RequestTarget,
            },
            None,
        )
        .expect_err("HTTP intent cannot silently succeed for Socket");
    assert_eq!(
        error.view_model.code,
        "LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE"
    );
}

#[test]
fn server_tls_transition_rejects_local_responder_for_both_boolean_inputs() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let local_responder = ListenerDraft {
        data_plane: ListenerDataPlaneDraft::Socket(SocketRelayDraftSettings {
            topology: SocketTopologyDraft::LocalResponder(SocketLocalResponderTopologyDraft {
                downstream_security: SocketDownstreamSecurityDraft::Tcp,
            }),
            processing: SocketPayloadProcessingDraft::Scripted(ScriptedSocketProcessingDraft {
                package: Some(recommended_package()),
            }),
            ..SocketRelayDraftSettings::default()
        }),
        ..ListenerDraft::from(ProxyListener::default())
    };

    let actual = [true, false].map(|enabled| {
        application
            .listener_draft_transition(
                local_responder.clone(),
                ListenerDraftTransition::ServerTls { enabled },
                None,
            )
            .map(|_| "ok")
            .map_err(|error| error.view_model.code)
    });
    assert_eq!(
        actual,
        [
            Err("LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE".to_owned()),
            Err("LISTENER_DRAFT_TRANSITION_NOT_APPLICABLE".to_owned()),
        ]
    );
}

#[test]
fn socket_transitions_preserve_package_and_keep_tls_sides_independent() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let relay = ListenerDraft::from(full_socket_listener());
    let local = application
        .listener_draft_transition(
            relay.clone(),
            ListenerDraftTransition::SocketTopology {
                mode: SocketTopologyMode::LocalResponder,
            },
            None,
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(local_settings) = &local.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    assert!(matches!(
        local_settings.topology,
        SocketTopologyDraft::LocalResponder(_)
    ));
    assert_eq!(
        local_settings.processing,
        SocketPayloadProcessingDraft::Scripted(ScriptedSocketProcessingDraft {
            package: Some(recommended_package()),
        })
    );

    let relay_again = application
        .listener_draft_transition(
            local,
            ListenerDraftTransition::SocketTopology {
                mode: SocketTopologyMode::Relay,
            },
            None,
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(settings) = relay_again.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    let SocketTopologyDraft::Relay(topology) = settings.topology else {
        panic!("Relay transition must create Relay topology");
    };
    assert_eq!(topology.upstream, SocketEndpoint::default());
    assert!(matches!(
        topology.security,
        SocketRelaySecurityDraft::TlsToTcp { .. }
    ));

    let direct = ListenerDraft::from(ProxyListener {
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings::default()),
        ..ProxyListener::default()
    });
    let scripted = application
        .listener_draft_transition(
            direct,
            ListenerDraftTransition::Processing {
                mode: SocketProcessingMode::Scripted,
            },
            Some(recommended_package()),
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(settings) = scripted.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    assert_eq!(
        settings.processing,
        SocketPayloadProcessingDraft::Scripted(ScriptedSocketProcessingDraft {
            package: Some(recommended_package()),
        })
    );
}

#[test]
fn socket_tls_transition_matrix_uses_defaults_and_changes_only_one_side() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let initial = ListenerDraft::from(ProxyListener {
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings::default()),
        ..ProxyListener::default()
    });
    let app_tls = application
        .listener_draft_transition(
            initial,
            ListenerDraftTransition::AppTls { enabled: true },
            None,
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(app_settings) = &app_tls.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    let SocketTopologyDraft::Relay(app_relay) = &app_settings.topology else {
        panic!("TLS transition must retain Relay topology");
    };
    assert_eq!(
        app_relay.security,
        SocketRelaySecurityDraft::TlsToTcp {
            downstream_tls: SocketDownstreamTlsDraftSettings {
                server_identity: None,
                client_authentication: DownstreamClientAuthenticationDraft::Disabled,
            },
        }
    );

    let both_tls = application
        .listener_draft_transition(
            app_tls,
            ListenerDraftTransition::ServerTls { enabled: true },
            None,
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(both_settings) = &both_tls.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    let SocketTopologyDraft::Relay(both_relay) = &both_settings.topology else {
        panic!("TLS transition must retain Relay topology");
    };
    assert!(matches!(
        both_relay.security,
        SocketRelaySecurityDraft::TlsToTls { .. }
    ));

    let server_only = application
        .listener_draft_transition(
            both_tls,
            ListenerDraftTransition::AppTls { enabled: false },
            None,
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(server_settings) = &server_only.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    let SocketTopologyDraft::Relay(server_relay) = &server_settings.topology else {
        panic!("TLS transition must retain Relay topology");
    };
    assert_eq!(
        server_relay.security,
        SocketRelaySecurityDraft::TcpToTls {
            upstream_tls: SocketUpstreamTlsSettings::default(),
        }
    );

    let transparent = application
        .listener_draft_transition(
            server_only,
            ListenerDraftTransition::ServerTls { enabled: false },
            None,
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(settings) = transparent.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    let SocketTopologyDraft::Relay(relay) = settings.topology else {
        panic!("TLS transition must retain Relay topology");
    };
    assert_eq!(relay.security, SocketRelaySecurityDraft::Transparent);
}

#[test]
fn socket_processing_and_topology_matrix_preserves_exact_package_rules() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let direct = ListenerDraft::from(ProxyListener {
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings::default()),
        ..ProxyListener::default()
    });
    let local = application
        .listener_draft_transition(
            direct,
            ListenerDraftTransition::SocketTopology {
                mode: SocketTopologyMode::LocalResponder,
            },
            None,
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(local_settings) = &local.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    assert_eq!(
        local_settings.processing,
        SocketPayloadProcessingDraft::Scripted(ScriptedSocketProcessingDraft { package: None })
    );

    let direct_relay = application
        .listener_draft_transition(
            local,
            ListenerDraftTransition::Processing {
                mode: SocketProcessingMode::Direct,
            },
            Some(recommended_package()),
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(settings) = &direct_relay.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    assert_eq!(settings.processing, SocketPayloadProcessingDraft::Direct);
    assert!(matches!(settings.topology, SocketTopologyDraft::Relay(_)));

    let existing = ListenerDraft::from(full_socket_listener());
    let local = application
        .listener_draft_transition(
            existing,
            ListenerDraftTransition::SocketTopology {
                mode: SocketTopologyMode::LocalResponder,
            },
            Some(ProtocolPackageRef {
                id: ProtocolPackageId::new("another-package").unwrap(),
                version: ProtocolPackageVersion::new("9.9.9").unwrap(),
            }),
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(settings) = local.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    assert_eq!(
        settings.processing,
        SocketPayloadProcessingDraft::Scripted(ScriptedSocketProcessingDraft {
            package: Some(recommended_package()),
        })
    );

    let inconsistent_local_direct = ListenerDraft {
        data_plane: ListenerDataPlaneDraft::Socket(SocketRelayDraftSettings {
            topology: SocketTopologyDraft::LocalResponder(SocketLocalResponderTopologyDraft {
                downstream_security: SocketDownstreamSecurityDraft::Tcp,
            }),
            processing: SocketPayloadProcessingDraft::Direct,
            ..SocketRelayDraftSettings::default()
        }),
        ..ListenerDraft::from(ProxyListener::default())
    };
    let normalized_direct = application
        .listener_draft_transition(
            inconsistent_local_direct,
            ListenerDraftTransition::Processing {
                mode: SocketProcessingMode::Direct,
            },
            None,
        )
        .unwrap();
    let ListenerDataPlaneDraft::Socket(settings) = normalized_direct.data_plane else {
        panic!("Socket transition must retain Socket data plane");
    };
    assert!(matches!(settings.topology, SocketTopologyDraft::Relay(_)));
}
