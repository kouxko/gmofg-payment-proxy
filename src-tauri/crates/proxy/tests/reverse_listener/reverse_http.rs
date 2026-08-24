fn plain_capabilities(
    listener_id: &str,
) -> Arc<dyn intercept_proxy_runtime::HttpProtocolCapabilityFactory> {
    Arc::new(intercept_proxy_runtime::PlainHttpCapabilityFactory::new(
        "reverse-test-workspace",
        listener_id,
    ))
}

#[tokio::test]
async fn reverse_http_request_reports_ordinary_tls_evidence_to_pipeline() {
    let upstream_server = identity("ordinary-request-server", false);
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
    let expected_host = format!("Host: 127.0.0.1:{}\r\n", upstream_address.port());
    let upstream_task = tokio::spawn(async move {
        let (tcp, _) = upstream_listener.accept().await.unwrap();
        let mut stream = acceptor.accept(tcp).await.unwrap();
        let mut request = [0_u8; 256];
        let read = stream.read(&mut request).await.unwrap();
        assert!(request[..read].starts_with(b"GET /probe?mode=fixed-https HTTP/1.1\r\n"));
        assert!(
            String::from_utf8_lossy(&request[..read]).contains(&expected_host),
            "固定上游必须收到按配置 authority 改写后的 Host"
        );
        assert!(
            !request[..read]
                .windows(b"Host: client.invalid".len())
                .any(|window| window == b"Host: client.invalid"),
            "客户端原始 Host 不得泄漏到固定上游"
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let ports = Arc::new(SecurityPorts::default());
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        allowed_client_cidrs: Vec::new(),
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
    .unwrap()
    .with_pipeline(
        intercept_proxy_runtime::ChannelId::new("ordinary-tls").unwrap(),
        ports.clone(),
        plain_capabilities("ordinary-tls"),
        MessageLimits::default(),
        4,
    )
    .unwrap();
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    let mut client = TcpStream::connect(reverse_address).await.unwrap();
    client
        .write_all(
            b"GET /probe?mode=fixed-https HTTP/1.1\r\nHost: client.invalid\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));

    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    let evidence = ports.evidence.lock().unwrap();
    assert_ordinary_tls_evidence(&evidence);
}

fn assert_ordinary_tls_evidence(evidence: &[UpstreamSecurityEvidence]) {
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].transport, UpstreamTransportSecurity::Tls);
    assert_eq!(evidence[0].tls_version.as_deref(), Some("TLS 1.2"));
    assert!(
        evidence[0]
            .peer_subject
            .as_deref()
            .unwrap()
            .contains("ordinary-request-server")
    );
    assert_eq!(evidence[0].hostname_verification_enabled, Some(true));
    assert!(!evidence[0].client_identity_configured);
    assert!(!evidence[0].client_identity_submitted);
}

#[tokio::test]
async fn reverse_http_request_rewrites_ipv6_authority_host() {
    let upstream_listener = TcpListener::bind((Ipv6Addr::LOCALHOST, 0))
        .await
        .expect("测试环境必须支持 IPv6 loopback");
    let upstream_address = upstream_listener.local_addr().unwrap();
    let expected_host = format!("Host: [::1]:{}\r\n", upstream_address.port());
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream_listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0_u8; 1];
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        let request = String::from_utf8(request).unwrap();
        assert!(
            request.contains(&expected_host),
            "IPv6 固定上游必须收到带方括号和非默认端口的 Host，实际请求：{request:?}"
        );
        assert!(!request.contains("Host: client.invalid"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        allowed_client_cidrs: Vec::new(),
        upstream_origin: format!("http://[::1]:{}", upstream_address.port()),
        downstream_tls: None,
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap()
    .with_pipeline(
        intercept_proxy_runtime::ChannelId::new("ipv6-host-rewrite").unwrap(),
        Arc::new(NoopPipelinePorts),
        plain_capabilities("ipv6-host-rewrite"),
        MessageLimits::default(),
        4,
    )
    .unwrap();
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    let mut client = TcpStream::connect(reverse_address).await.unwrap();
    client
        .write_all(b"GET /ipv6 HTTP/1.1\r\nHost: client.invalid\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));

    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn fixed_http_preserves_methods_query_and_body() {
    let cases = [
        ("GET", "/fixed?kind=get", ""),
        ("POST", "/fixed?kind=post", "post-body"),
        ("PUT", "/fixed?kind=put", "put-body"),
        ("DELETE", "/fixed?kind=delete", ""),
        ("QUERY", "/fixed?kind=query&cursor=next", "query-body"),
    ];
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        for (method, path_and_query, expected_body) in cases {
            let (mut stream, _) = upstream_listener.accept().await.unwrap();
            let mut request = Vec::new();
            let header_end = loop {
                if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
                let mut buffer = [0_u8; 512];
                let read = stream.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buffer[..read]);
            };
            let headers = std::str::from_utf8(&request[..header_end]).unwrap();
            assert!(
                headers.starts_with(&format!("{method} {path_and_query} HTTP/1.1\r\n")),
                "actual headers: {headers:?}"
            );
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(str::trim)
                        .map(str::parse::<usize>)
                })
                .transpose()
                .unwrap()
                .unwrap_or(0);
            while request.len() - header_end < content_length {
                let mut buffer = [0_u8; 512];
                let read = stream.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buffer[..read]);
            }
            assert_eq!(
                &request[header_end..header_end + content_length],
                expected_body.as_bytes()
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
        }
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        allowed_client_cidrs: Vec::new(),
        upstream_origin: format!("http://{upstream_address}"),
        downstream_tls: None,
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap()
    .with_pipeline(
        intercept_proxy_runtime::ChannelId::new("fixed-methods").unwrap(),
        Arc::new(NoopPipelinePorts),
        plain_capabilities("fixed-methods"),
        MessageLimits::default(),
        4,
    )
    .unwrap();
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    for (method, path_and_query, body) in cases {
        let mut client = TcpStream::connect(reverse_address).await.unwrap();
        let request = format!(
            "{method} {path_and_query} HTTP/1.1\r\nHost: proxy.invalid\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        client.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    }
    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
}
