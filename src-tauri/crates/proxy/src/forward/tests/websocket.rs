#[tokio::test]
async fn websocket_handshake_enters_pipeline_then_frames_tunnel_transparently() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let target_task = tokio::spawn(async move {
        let (mut stream, _) = target.accept().await.unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        assert!(request.starts_with(b"GET /socket HTTP/1.1\r\n"));
        stream
            .write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
            )
            .await
            .unwrap();
        let mut ping = [0u8; 4];
        stream.read_exact(&mut ping).await.unwrap();
        assert_eq!(&ping, b"ping");
        stream.write_all(b"pong").await.unwrap();
    });

    let ports = Arc::new(CapturingPipelinePorts::default());
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_pipeline(
            ChannelId::new("websocket-test").unwrap(),
            Uuid::new_v4(),
            ports.clone(),
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(16 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45006".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection.with_upgrades());
    let mut response = sender
        .send_request(
            Request::builder()
                .uri(format!("http://{target_address}/socket"))
                .header(CONNECTION, "Upgrade")
                .header(UPGRADE, "websocket")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    let upgraded = hyper::upgrade::on(&mut response).await.unwrap();
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
    assert_eq!(ports.requests.lock().unwrap().len(), 1);
    assert_eq!(ports.responses.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn websocket_drop_response_is_rejected_before_upgrade() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let ports = Arc::new(CapturingPipelinePorts {
        request_actions: vec![FaultAction::DropResponse {
            read_upstream: false,
        }],
        ..Default::default()
    });
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_pipeline(
            ChannelId::new("websocket-drop-test").unwrap(),
            Uuid::new_v4(),
            ports,
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(16 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45103".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection.with_upgrades());
    let response = sender
        .send_request(
            Request::builder()
                .uri(format!("http://{target_address}/socket"))
                .header(CONNECTION, "Upgrade")
                .header(UPGRADE, "websocket")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("not supported for WebSocket Upgrade"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), target.accept())
            .await
            .is_err(),
        "rejected WebSocket action must not open the origin connection"
    );
    drop(sender);
    connection_task.await.unwrap().unwrap();
    proxy_task.await.unwrap().unwrap();
}
