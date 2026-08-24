#[tokio::test]
async fn connect_is_rejected_without_dialing_the_requested_target() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication)).unwrap();
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
    let connection_task = tokio::spawn(connection);
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
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), target.accept())
            .await
            .is_err(),
        "unsupported CONNECT must not dial the requested target"
    );
    drop(sender);
    connection_task.await.unwrap().unwrap();
    proxy_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn websocket_upgrade_is_rejected_without_dialing_the_origin() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication)).unwrap();
    let (client, proxy) = tokio::io::duplex(16 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45007".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection);
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
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), target.accept())
            .await
            .is_err(),
        "unsupported Upgrade must not dial the requested origin"
    );
    drop(sender);
    connection_task.await.unwrap().unwrap();
    proxy_task.await.unwrap().unwrap();
}
