#[tokio::test]
async fn upstream_tls_test_uses_listener_ca_hostname_and_client_identity() {
    let upstream_server = identity("probe-upstream-server", false);
    let upstream_client = identity("probe-upstream-client", true);
    let upstream_tls = ServerTlsAdapter::build(
        vec![upstream_server.cert.clone(), upstream_server.ca.clone()],
        upstream_server.key.clone(),
        upstream_client.ca.clone(),
        None,
        Arc::new(NoopPipelinePorts),
    )
    .unwrap();
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (tcp, peer) = upstream_listener.accept().await.unwrap();
        let accepted = upstream_tls
            .accept(Box::new(tcp), &context(peer.port()))
            .await
            .unwrap();
        assert!(accepted.tls_peer.is_some(), "Server 必须收到客户端证书");
    });

    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: (Ipv4Addr::LOCALHOST, 0).into(),
        upstream_origin: format!("https://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: None,
        upstream_tls: Some(ReverseUpstreamTls {
            server_trust_der: vec![upstream_server.ca],
            client_identity: Some(ReverseClientIdentity {
                certificate_chain_der: vec![upstream_client.cert, upstream_client.ca],
                private_key_pkcs8_der: upstream_client.key.into(),
            }),
            verify_hostname: true,
        }),
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();

    let result = service.test_upstream_tls().await.unwrap();
    assert_eq!(result.resolved_address, upstream_address);
    assert_eq!(result.tls_version, "TLS 1.2");
    assert!(result.cipher_suite.starts_with("TLS_"));
    assert!(result.peer_subject.contains("probe-upstream-server"));
    assert!(!result.peer_sha256_fingerprint.is_empty());
    assert!(result.hostname_verification_enabled);
    assert!(result.client_identity_configured);
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn upstream_tls_test_supports_ordinary_tls_without_a_client_identity() {
    let upstream_server = identity("ordinary-tls-server", false);
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS12])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![
                    CertificateDer::from(upstream_server.cert.clone()),
                    CertificateDer::from(upstream_server.ca.clone()),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(upstream_server.key)),
            )
            .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (tcp, _) = upstream_listener.accept().await.unwrap();
        acceptor.accept(tcp).await.unwrap();
    });

    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: (Ipv4Addr::LOCALHOST, 0).into(),
        upstream_origin: format!("https://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: None,
        upstream_tls: Some(ReverseUpstreamTls {
            server_trust_der: vec![upstream_server.ca],
            client_identity: None,
            verify_hostname: true,
        }),
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();

    let result = service.test_upstream_tls().await.unwrap();
    assert_eq!(result.tls_version, "TLS 1.2");
    assert!(!result.client_identity_configured);
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn upstream_connection_test_uses_tcp_only_for_http() {
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut tcp, _) = upstream_listener.accept().await.unwrap();
        let mut byte = [0_u8; 1];
        assert_eq!(tcp.read(&mut byte).await.unwrap(), 0);
    });
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: (Ipv4Addr::LOCALHOST, 0).into(),
        upstream_origin: format!("http://{upstream_address}"),
        downstream_tls: None,
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();

    let result = service.test_upstream_connection().await.unwrap();
    assert_eq!(result.resolved_address, upstream_address);
    assert_eq!(result.scheme, UpstreamScheme::Http);
    assert_eq!(result.transport, UpstreamTransport::Tcp);
    assert!(result.tls.is_none());
    upstream_task.await.unwrap();
}
