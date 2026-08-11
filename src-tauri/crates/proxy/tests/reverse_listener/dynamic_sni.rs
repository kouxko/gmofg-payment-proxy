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

#[tokio::test]
async fn reverse_listener_uses_fallback_identity_when_client_sends_ip_without_sni() {
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
                certificate_chain_der: vec![fallback.cert, fallback.ca.clone()],
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

    // rustls 对 IP 类型 ServerName 不发送 SNI；此时必须退回监听器固定证书，
    // 不能因为动态域名证书启用就直接关闭连接。
    let client = ClientTlsAdapter::build(
        vec![downstream_client.cert, downstream_client.ca],
        downstream_client.key,
        fallback.ca,
    )
    .unwrap();
    let tcp = TcpStream::connect(reverse_address).await.unwrap();
    let mut stream = client.connect("127.0.0.1", Box::new(tcp)).await.unwrap();
    stream.shutdown().await.unwrap();

    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    assert!(authority.issued_names.lock().unwrap().is_empty());
}

#[tokio::test]
async fn dynamic_ecdsa_identity_accepts_android_compatible_tls12_client_hello() {
    let authority = Arc::new(DynamicAuthority::new());
    let fallback = identity("fallback-server", false);
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

    // Android 11 Conscrypt 支持的核心 TLS 1.2 ECDSA 签名组合。将声明范围
    // 收窄到该组合，可避免“默认客户端能力过宽”掩盖兼容性问题。
    let mut stream = connect_with_signature_schemes(
        reverse_address,
        "https.gmo-fg.net",
        authority.ca_der.clone(),
        &[SignatureScheme::ECDSA_NISTP256_SHA256],
    )
    .await
    .unwrap();
    stream.shutdown().await.unwrap();

    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    assert_eq!(
        authority.issued_names.lock().unwrap().as_slice(),
        ["https.gmo-fg.net"]
    );
}

#[tokio::test]
async fn dynamic_ecdsa_identity_rejects_client_hello_without_ecdsa_signature_scheme() {
    let authority = Arc::new(DynamicAuthority::new());
    let fallback = identity("fallback-server", false);
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

    let result = connect_with_signature_schemes(
        reverse_address,
        "https.gmo-fg.net",
        authority.ca_der.clone(),
        &[SignatureScheme::RSA_PKCS1_SHA256],
    )
    .await;
    assert!(result.is_err(), "RSA-only ClientHello 不应匹配 ECDSA 叶子证书");

    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    assert_eq!(
        authority.issued_names.lock().unwrap().as_slice(),
        ["https.gmo-fg.net"],
        "解析 SNI 后会先签发叶子证书，随后因客户端未声明 ECDSA 而拒绝握手"
    );
}
