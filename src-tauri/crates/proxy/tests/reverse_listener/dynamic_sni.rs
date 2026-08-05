#[tokio::test]
async fn concurrent_connections_issue_one_dynamic_identity_per_sni() {
    const CLIENTS: u8 = 8;
    let authority = Arc::new(DynamicAuthority::new());
    let fallback = identity("fallback-server", false);
    let downstream_client = identity("ordinary-client", true);
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..usize::from(CLIENTS) {
            let (mut stream, _) = upstream_listener.accept().await.unwrap();
            tasks.spawn(async move {
                let mut request = [0_u8; 1];
                stream.read_exact(&mut request).await.unwrap();
                stream.write_all(&request).await.unwrap();
                stream.shutdown().await.unwrap();
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        allowed_client_cidrs: Vec::new(),
        upstream_origin: format!("http://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: Some(ReverseDownstreamTls {
            server_identity: ReverseClientIdentity {
                certificate_chain_der: vec![fallback.cert, fallback.ca],
                private_key_pkcs8_der: fallback.key.into(),
            },
            dynamic_server_identity: Some(authority.clone()),
            dynamic_server_name_allowlist: vec!["shared.example.test".into()],
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

    let mut clients = tokio::task::JoinSet::new();
    for byte in 0..CLIENTS {
        let certificate_chain = vec![downstream_client.cert.clone(), downstream_client.ca.clone()];
        let private_key = downstream_client.key.clone();
        let root = authority.ca_der.clone();
        clients.spawn(async move {
            let client = ClientTlsAdapter::build(certificate_chain, private_key, root).unwrap();
            let tcp = TcpStream::connect(reverse_address).await.unwrap();
            let mut stream = client
                .connect("shared.example.test", Box::new(tcp))
                .await
                .unwrap();
            stream.write_all(&[byte]).await.unwrap();
            let mut response = [0_u8; 1];
            stream.read_exact(&mut response).await.unwrap();
            assert_eq!(response, [byte]);
        });
    }
    while let Some(result) = clients.join_next().await {
        result.unwrap();
    }

    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    assert_eq!(
        authority
            .issued_names
            .lock()
            .unwrap()
            .iter()
            .filter(|name| name.as_str() == "shared.example.test")
            .count(),
        1
    );
}

#[tokio::test]
async fn reverse_listener_rejects_sni_outside_dynamic_allowlist() {
    let authority = Arc::new(DynamicAuthority::new());
    let fallback = identity("fallback-server", false);
    let downstream_client = identity("ordinary-client", true);
    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        allowed_client_cidrs: Vec::new(),
        upstream_origin: "http://127.0.0.1:9".into(),
        downstream_tls: Some(ReverseDownstreamTls {
            server_identity: ReverseClientIdentity {
                certificate_chain_der: vec![fallback.cert, fallback.ca],
                private_key_pkcs8_der: fallback.key.into(),
            },
            dynamic_server_identity: Some(authority.clone()),
            dynamic_server_name_allowlist: vec!["allowed.example.test".into()],
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
    assert!(
        client
            .connect("blocked.example.test", Box::new(tcp))
            .await
            .is_err()
    );

    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    assert!(authority.issued_names.lock().unwrap().is_empty());
}
