#[tokio::test]
async fn one_app_connection_fails_when_a_later_request_changes_endpoint() {
    let first_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let first_address = first_origin.local_addr().unwrap();
    let second_origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_address = second_origin.local_addr().unwrap();
    let first_task = tokio::spawn(async move {
        let (stream, _) = first_origin.accept().await.unwrap();
        server_http1::Builder::new()
            .serve_connection(
                TokioIo::new(stream),
                service_fn(|_request: Request<Incoming>| async move {
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"first"))))
                }),
            )
            .await
            .unwrap();
    });
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_pipeline(
            ChannelId::new("changed-endpoint").unwrap(),
            Uuid::new_v4(),
            Arc::new(CapturingPipelinePorts::default()),
            plain_capabilities("changed-endpoint"),
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(16 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45019".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection);
    let first = sender
        .send_request(
            Request::builder()
                .uri(format!("http://{first_address}/one"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        first.into_body().collect().await.unwrap().to_bytes(),
        "first"
    );

    let second = sender
        .send_request(
            Request::builder()
                .uri(format!("http://{second_address}/two"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await;
    assert!(
        second.is_err(),
        "endpoint changes must fail the connection Exchange"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), second_origin.accept())
            .await
            .is_err(),
        "endpoint mismatch must be rejected before dialing another Server"
    );
    drop(sender);
    let _ = connection_task.await;
    assert!(proxy_task.await.unwrap().is_err());
    first_task.await.unwrap();
}

#[tokio::test]
async fn capability_factory_panic_fails_and_drains_the_connection_exchange() {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_pipeline(
            ChannelId::new("panic-capability").unwrap(),
            Uuid::new_v4(),
            Arc::new(CapturingPipelinePorts::default()),
            Arc::new(PanickingHttpCapabilities),
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(16 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45029".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection);
    let response = sender
        .send_request(
            Request::builder()
                .uri(format!("http://{origin_address}/panic"))
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await;
    assert!(response.is_err(), "factory panic must terminate the App connection");
    assert!(
        tokio::time::timeout(Duration::from_millis(100), origin.accept())
            .await
            .is_err(),
        "factory panic must fail before Server dial"
    );
    drop(sender);
    let _ = connection_task.await;
    assert!(proxy_task.await.unwrap().is_err());
}
