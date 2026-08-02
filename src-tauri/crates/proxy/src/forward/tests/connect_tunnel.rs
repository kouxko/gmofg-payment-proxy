#[tokio::test]
async fn tunnel_copies_both_directions_and_half_closes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"ping");
        stream.write_all(b"pong").await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let upstream = TcpStream::connect(address).await.unwrap();
    let (mut client, proxy) = tokio::io::duplex(1024);
    let tunnel = tokio::spawn(run_tunnel(
        proxy,
        upstream,
        Duration::from_secs(1),
        CancellationToken::new(),
    ));
    client.write_all(b"ping").await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, b"pong");
    tunnel.await.unwrap().unwrap();
    server.await.unwrap();
}

async fn connect_tunnel_round_trip_once() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let target_task = tokio::spawn(async move {
        let (mut stream, _) = target.accept().await.unwrap();
        let mut request = [0u8; 4];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"ping");
        stream.write_all(b"pong").await.unwrap();
    });

    let certificate_authority = Arc::new(CountingCertificateAuthority::default());
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_mitm(
            ForwardMitmConfig {
                authority_allowlist: vec!["allowed.example.test".into()],
                maximum_cached_leaf_certificates: 8,
            },
            certificate_authority.clone(),
            Arc::new(NeverMitmUpstreamConnector),
        )
        .unwrap();
    let (client, proxy) = tokio::io::duplex(16 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45001".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection.with_upgrades());
    let request = Request::builder()
        .method(Method::CONNECT)
        .uri(target_address.to_string())
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let upgraded = hyper::upgrade::on(response).await.unwrap();
    let mut tunnel = TokioIo::new(upgraded);
    tunnel.write_all(b"ping").await.unwrap();
    let mut reply = [0u8; 4];
    tunnel.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"pong");
    drop(tunnel);
    drop(sender);
    connection_task.await.unwrap().unwrap();
    proxy_task.await.unwrap().unwrap();
    target_task.await.unwrap();
    assert_eq!(
        certificate_authority.count(),
        0,
        "allowlist-excluded CONNECT must remain a transparent tunnel"
    );
}

#[tokio::test]
async fn connect_tunnel_is_stable_for_one_hundred_consecutive_connections() {
    for _ in 0..100 {
        connect_tunnel_round_trip_once().await;
    }
}

#[tokio::test]
async fn allowlisted_h2_client_hello_stays_transparent_and_skips_pipeline() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let client_hello = fragmented_client_hello_with_alpn(b"h2");
    let expected_client_hello = client_hello.clone();
    let target_task = tokio::spawn(async move {
        let (mut stream, _) = target.accept().await.unwrap();
        let mut actual_client_hello = vec![0; expected_client_hello.len()];
        stream.read_exact(&mut actual_client_hello).await.unwrap();
        assert_eq!(actual_client_hello, expected_client_hello);
        stream.write_all(b"opaque-h2").await.unwrap();
    });

    let certificate_authority = Arc::new(CountingCertificateAuthority::default());
    let ports = Arc::new(CapturingPipelinePorts::default());
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_mitm(
            ForwardMitmConfig {
                authority_allowlist: vec!["127.0.0.1".into()],
                maximum_cached_leaf_certificates: 8,
            },
            certificate_authority,
            Arc::new(NeverMitmUpstreamConnector),
        )
        .unwrap()
        .with_pipeline(
            ChannelId::new("h2-tunnel-test").unwrap(),
            Uuid::new_v4(),
            ports.clone(),
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(16 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45004".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection.with_upgrades());
    let response = sender
        .send_request(
            Request::builder()
                .method(Method::CONNECT)
                .uri(target_address.to_string())
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let upgraded = hyper::upgrade::on(response).await.unwrap();
    let mut tunnel = TokioIo::new(upgraded);
    tunnel.write_all(&client_hello).await.unwrap();
    let mut reply = [0u8; 9];
    tunnel.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"opaque-h2");
    drop(tunnel);
    drop(sender);
    connection_task.await.unwrap().unwrap();
    proxy_task.await.unwrap().unwrap();
    target_task.await.unwrap();
    assert!(ports.requests.lock().unwrap().is_empty());
    assert!(ports.responses.lock().unwrap().is_empty());
}
