#[tokio::test]
async fn reverse_listener_negotiates_mtls_on_both_sides_and_preserves_body_bytes() {
    let downstream_server = identity("reverse-server", false);
    let downstream_client = identity("application-client", true);
    let upstream_server = identity("upstream-server", false);
    let upstream_client = identity("reverse-upstream-client", true);

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
        let mut accepted = upstream_tls
            .accept(Box::new(tcp), &context(peer.port()))
            .await
            .unwrap();
        assert!(accepted.tls_peer.take().is_some());
        let mut request = [0_u8; 8];
        accepted.io.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"\x00request");
        accepted.io.write_all(b"\x81response\xff").await.unwrap();
        accepted.io.shutdown().await.unwrap();
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        upstream_origin: format!("https://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: Some(ReverseDownstreamTls {
            server_identity: ReverseClientIdentity {
                certificate_chain_der: vec![
                    downstream_server.cert.clone(),
                    downstream_server.ca.clone(),
                ],
                private_key_pkcs8_der: downstream_server.key.clone().into(),
            },
            dynamic_server_identity: None,
            dynamic_server_name_allowlist: Vec::new(),
            client_trust_der: vec![downstream_client.ca.clone()],
            client_authentication_required: true,
        }),
        upstream_tls: Some(ReverseUpstreamTls {
            server_trust_der: vec![upstream_server.ca.clone()],
            client_identity: Some(ReverseClientIdentity {
                certificate_chain_der: vec![
                    upstream_client.cert.clone(),
                    upstream_client.ca.clone(),
                ],
                private_key_pkcs8_der: upstream_client.key.clone().into(),
            }),
            verify_hostname: true,
        }),
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    let downstream_tls = ClientTlsAdapter::build(
        vec![downstream_client.cert, downstream_client.ca],
        downstream_client.key,
        downstream_server.ca,
    )
    .unwrap();
    let tcp = TcpStream::connect(reverse_address).await.unwrap();
    let mut client = downstream_tls
        .connect("127.0.0.1", Box::new(tcp))
        .await
        .unwrap();
    client.write_all(b"\x00request").await.unwrap();
    let mut response = [0_u8; 10];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(&response, b"\x81response\xff");

    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_silent_reverse_tls_handshake_drains_blocking_owner_without_starving_runtime() {
    let downstream_server = identity("reverse-server", false);
    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        upstream_origin: "http://127.0.0.1:9".into(),
        downstream_tls: Some(ReverseDownstreamTls {
            server_identity: ReverseClientIdentity {
                certificate_chain_der: vec![downstream_server.cert, downstream_server.ca],
                private_key_pkcs8_der: downstream_server.key.into(),
            },
            dynamic_server_identity: None,
            dynamic_server_name_allowlist: Vec::new(),
            client_trust_der: Vec::new(),
            client_authentication_required: false,
        }),
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });
    let _silent_client = TcpStream::connect(reverse_address).await.unwrap();

    let (progress_tx, progress_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move { progress_tx.send(()).unwrap() });
    progress_rx
        .await
        .expect("current-thread runtime progresses during silent TLS handshake");

    cancellation.cancel();
    tokio::time::timeout(Duration::from_secs(1), reverse_task)
        .await
        .expect("reverse TLS blocking owner drains after cancellation")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn reverse_listener_without_explicit_identity_issues_leaf_for_client_sni() {
    let authority = Arc::new(DynamicAuthority::new());
    let fallback = identity("fallback-server", false);
    let downstream_client = identity("ordinary-client", true);
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream_listener.accept().await.unwrap();
        let mut request = [0_u8; 7];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"request");
        stream.write_all(b"response").await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        upstream_origin: format!("http://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: Some(ReverseDownstreamTls {
            server_identity: ReverseClientIdentity {
                certificate_chain_der: vec![fallback.cert, fallback.ca],
                private_key_pkcs8_der: fallback.key.into(),
            },
            dynamic_server_identity: Some(authority.clone()),
            dynamic_server_name_allowlist: vec!["https.gmo-fg.net".into()],
            client_trust_der: Vec::new(),
            client_authentication_required: false,
        }),
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    let client = ClientTlsAdapter::build(
        vec![downstream_client.cert, downstream_client.ca],
        downstream_client.key,
        authority.ca_der.clone(),
    )
    .unwrap();
    let tcp = TcpStream::connect(reverse_address).await.unwrap();
    let mut stream = client
        .connect("https.gmo-fg.net", Box::new(tcp))
        .await
        .unwrap();
    stream.write_all(b"request").await.unwrap();
    let mut response = [0_u8; 8];
    stream.read_exact(&mut response).await.unwrap();
    assert_eq!(&response, b"response");

    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    assert_eq!(
        authority.issued_names.lock().unwrap().as_slice(),
        ["https.gmo-fg.net"]
    );
}
