use intercept_proxy_application::{ListenerDataPlaneKind, SocketTransportMode};
use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, CertificateReferenceKind,
    DirectionProcessingOptions, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    ScriptedSocketProcessing, SocketDownstreamSecurity, SocketDownstreamTlsSettings,
    SocketEndpoint, SocketLocalResponderTopology, SocketPayloadProcessing, SocketRelaySecurity,
    SocketRelaySettings, SocketTopology, SocketUpstreamTlsSettings,
};

fn socket_listener(bind: SocketAddr, upstream: SocketAddr) -> ProxyListener {
    ProxyListener {
        id: ListenerId::new(),
        name: "transparent socket".into(),
        enabled: false,
        bind_address: bind.ip().to_string(),
        port: bind.port(),
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings::relay(
            SocketEndpoint {
                host: upstream.ip().to_string(),
                port: upstream.port(),
            },
            SocketRelaySecurity::Transparent,
            8,
            SocketPayloadProcessing::Direct,
        )),
        ..ProxyListener::default()
    }
}

fn local_responder_listener(bind: SocketAddr) -> ProxyListener {
    ProxyListener {
        id: ListenerId::new(),
        name: "local responder".into(),
        enabled: false,
        bind_address: bind.ip().to_string(),
        port: bind.port(),
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
            topology: SocketTopology::LocalResponder(SocketLocalResponderTopology {
                downstream_security: SocketDownstreamSecurity::Tcp,
            }),
            maximum_connections: 8,
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: ProtocolPackageRef {
                    id: ProtocolPackageId::new("iso8583-standard").unwrap(),
                    version: ProtocolPackageVersion::new("1.2.3").unwrap(),
                },
                upstream: DirectionProcessingOptions {
                    decode_enabled: true,
                    encode_enabled: false,
                },
                downstream: DirectionProcessingOptions {
                    decode_enabled: false,
                    encode_enabled: true,
                },
            }),
        }),
        ..ProxyListener::default()
    }
}

#[tokio::test]
async fn local_responder_plan_requires_exact_package_but_upstream_probe_is_not_applicable() {
    let listener = local_responder_listener("127.0.0.1:19079".parse().unwrap());
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().expect("valid LocalResponder fixture");
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    let builder = ListenerRuntimePlanBuilder::new(&runtime);

    let start_error = builder
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .err()
        .expect("T21 LocalResponder plan must fresh-load its exact package");
    assert_eq!(
        start_error.view_model.code,
        "PROTOCOL_PACKAGE_NOT_FOUND"
    );
    let probe_error = builder
        .build_probe(&workspace, &listener, Uuid::new_v4())
        .await
        .err()
        .expect("LocalResponder has no upstream probe");
    assert_eq!(
        probe_error.view_model.code,
        "LISTENER_UPSTREAM_NOT_APPLICABLE"
    );
}

#[tokio::test]
async fn socket_probe_does_not_load_downstream_tls_identity() {
    let bind = "127.0.0.1:19083".parse().unwrap();
    let upstream = "127.0.0.1:19084".parse().unwrap();
    let identity_id = CertificateReferenceId::new();
    let mut listener = socket_listener(bind, upstream);
    listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings::relay(
        SocketEndpoint {
            host: upstream.ip().to_string(),
            port: upstream.port(),
        },
        SocketRelaySecurity::TlsToTcp {
            downstream_tls: SocketDownstreamTlsSettings {
                server_identity: identity_id,
                client_authentication: DownstreamClientAuthentication::Disabled,
            },
        },
        8,
        SocketPayloadProcessing::Direct,
    ));
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        certificate_references: vec![CertificateReference {
            id: identity_id,
            label: "not managed".into(),
            kind: CertificateReferenceKind::ReverseServerIdentity,
            reference: "external:not-managed".into(),
        }],
        ..ProxyWorkspace::default()
    };
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));

    ListenerRuntimePlanBuilder::new(&runtime)
        .build_probe(&workspace, &listener, Uuid::new_v4())
        .await
        .expect("upstream TCP probe does not load downstream TLS identity");
    let error = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .err()
        .expect("listener start must load its selected downstream TLS identity");
    assert_eq!(
        error.view_model.code,
        "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
    );
}

#[tokio::test]
async fn transparent_socket_listener_starts_relays_metrics_and_releases_port() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = [0_u8; 7];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"\0socket");
        stream.write_all(b"reply\xff").await.unwrap();
    });
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind_address = reservation.local_addr().unwrap();
    drop(reservation);
    let listener = socket_listener(bind_address, upstream_address);
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));

    runtime
        .start(workspace, listener.clone())
        .await
        .expect("Socket listener starts after binding its port");
    let mut client = TcpStream::connect(bind_address).await.unwrap();
    client.write_all(b"\0socket").await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, b"reply\xff");
    upstream_task.await.unwrap();

    let status = runtime.statuses().await.unwrap().pop().unwrap();
    assert_eq!(status.client_to_server_bytes, 7);
    assert_eq!(status.server_to_client_bytes, 6);
    assert_eq!(status.retained_diagnostic_evictions, 0);
    runtime.stop(listener.id).await.unwrap();
    TcpListener::bind(bind_address)
        .await
        .expect("stopped Socket listener releases its port");
}

#[tokio::test]
async fn socket_plan_resolves_only_references_selected_by_its_tls_mode() {
    let bind = "127.0.0.1:19080".parse().unwrap();
    let upstream = "127.0.0.1:19081".parse().unwrap();
    let reference_id = CertificateReferenceId::new();
    let mut listener = socket_listener(bind, upstream);
    let workspace_with = |listener: ProxyListener| ProxyWorkspace {
        listeners: vec![listener],
        certificate_references: vec![CertificateReference {
            id: reference_id,
            label: "not managed".into(),
            kind: CertificateReferenceKind::UpstreamServerTrust,
            reference: "external:not-managed".into(),
        }],
        ..ProxyWorkspace::default()
    };
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    let transparent_workspace = workspace_with(listener.clone());
    ListenerRuntimePlanBuilder::new(&runtime)
        .build(&transparent_workspace, &listener, Uuid::new_v4())
        .await
        .expect("Transparent mode does not resolve unrelated certificate references");

    listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings::relay(
        SocketEndpoint {
            host: upstream.ip().to_string(),
            port: upstream.port(),
        },
        SocketRelaySecurity::TcpToTls {
            upstream_tls: SocketUpstreamTlsSettings::default(),
        },
        8,
        SocketPayloadProcessing::Direct,
    ));
    let system_trust_workspace = workspace_with(listener.clone());
    ListenerRuntimePlanBuilder::new(&runtime)
        .build(&system_trust_workspace, &listener, Uuid::new_v4())
        .await
        .expect("TCP-to-TLS resolves only configured upstream roles and no HTTP pipeline");

    listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings::relay(
        SocketEndpoint {
            host: upstream.ip().to_string(),
            port: upstream.port(),
        },
        SocketRelaySecurity::TcpToTls {
            upstream_tls: SocketUpstreamTlsSettings {
                verify_hostname: true,
                server_trust: Some(reference_id),
                client_identity: None,
            },
        },
        8,
        SocketPayloadProcessing::Direct,
    ));
    let selected_workspace = workspace_with(listener.clone());
    let error = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&selected_workspace, &listener, Uuid::new_v4())
        .await
        .err()
        .expect("selected unmanaged trust reference must fail closed");
    assert_eq!(
        error.view_model.code,
        "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
    );
}

#[tokio::test]
async fn socket_connection_probe_reports_plain_transport_and_mode() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let accepted = tokio::spawn(async move { upstream.accept().await.unwrap() });
    let listener = socket_listener("127.0.0.1:19082".parse().unwrap(), upstream_address);
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));

    let result = runtime
        .test_upstream_connection(workspace, listener)
        .await
        .unwrap();

    assert_eq!(result.data_plane, ListenerDataPlaneKind::Socket);
    assert_eq!(result.transport, "tcp");
    assert_eq!(
        result.socket_transport_mode,
        Some(SocketTransportMode::Transparent)
    );
    assert!(result.tls.is_none());
    accepted.await.unwrap();
}
