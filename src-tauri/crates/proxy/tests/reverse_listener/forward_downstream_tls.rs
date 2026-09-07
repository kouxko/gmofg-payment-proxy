async fn forward_downstream_tls_round_trip(require_client_identity: bool) {
    let server = identity("forward-downstream-server", false);
    let client = identity("forward-downstream-client", true);
    let origin = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_task = tokio::spawn(async move {
        let (mut stream, _) = origin.accept().await.unwrap();
        let mut request = [0_u8; 512];
        let read = stream.read(&mut request).await.unwrap();
        assert!(request[..read].starts_with(b"PUT /secure?mode=forward HTTP/1.1\r\n"));
        assert!(request[..read].ends_with(b"tls-body"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
            .unwrap();
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let listener_address = listener.local_addr().unwrap();
    let service = ForwardProxyService::new(
        ForwardProxyConfig {
            bind_addr: listener_address,
            authentication: ForwardAuthenticationMode::None,
            connect_timeout: Duration::from_secs(2),
            read_timeout: Duration::from_secs(2),
            write_timeout: Duration::from_secs(2),
        },
        Arc::new(NoAuthentication),
    )
    .unwrap()
    .with_downstream_tls(&ReverseDownstreamTls {
        server_identity: ReverseClientIdentity {
            certificate_chain_der: vec![server.cert.clone(), server.ca.clone()],
            private_key_pkcs8_der: server.key.into(),
        },
        dynamic_server_identity: None,
        dynamic_server_name_allowlist: Vec::new(),
        client_trust_der: require_client_identity
            .then(|| client.ca.clone())
            .into_iter()
            .collect(),
        client_authentication_required: require_client_identity,
    })
    .unwrap();
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let service_task =
        tokio::spawn(async move { service.serve_listener(listener, task_cancellation).await });

    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(server.ca)).unwrap();
    let builder =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS12])
            .unwrap()
            .with_root_certificates(roots);
    let config = if require_client_identity {
        builder
            .with_client_auth_cert(
                vec![
                    CertificateDer::from(client.cert),
                    CertificateDer::from(client.ca),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(client.key)),
            )
            .unwrap()
    } else {
        builder.with_no_client_auth()
    };
    let tcp = TcpStream::connect(listener_address).await.unwrap();
    let mut tls = TlsConnector::from(Arc::new(config))
        .connect(ServerName::IpAddress(Ipv4Addr::LOCALHOST.into()), tcp)
        .await
        .unwrap();
    let request = format!(
        "PUT http://{origin_address}/secure?mode=forward HTTP/1.1\r\nHost: {origin_address}\r\nContent-Length: 8\r\nConnection: close\r\n\r\ntls-body"
    );
    tls.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tls.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    origin_task.await.unwrap();
    cancellation.cancel();
    service_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn forward_listener_applies_downstream_tls() {
    forward_downstream_tls_round_trip(false).await;
}

#[tokio::test]
async fn forward_listener_applies_downstream_mutual_tls() {
    forward_downstream_tls_round_trip(true).await;
}
