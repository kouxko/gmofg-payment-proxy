async fn absolute_form_round_trip_once() {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_task = tokio::spawn(async move {
        let (mut stream, _) = origin.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /probe?q=1 http/1.1\r\n"));
        assert!(!request.contains("proxy-connection:"));
        assert!(!request.contains("x-private-hop:"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });

    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication)).unwrap();
    let (client, proxy) = tokio::io::duplex(16 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45000".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection);
    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("http://{origin_address}/probe?q=1"))
        .header("proxy-connection", "keep-alive")
        .header(CONNECTION, "x-private-hop")
        .header("x-private-hop", "remove")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "ok"
    );
    drop(sender);
    connection_task.await.unwrap().unwrap();
    proxy_task.await.unwrap().unwrap();
    origin_task.await.unwrap();
}

#[tokio::test]
async fn absolute_form_request_is_stable_for_one_hundred_consecutive_connections() {
    for _ in 0..100 {
        absolute_form_round_trip_once().await;
    }
}

#[tokio::test]
async fn absolute_form_preserves_standard_and_extension_methods_query_and_body() {
    let cases = [
        ("GET", "/method?kind=get", ""),
        ("POST", "/method?kind=post", "post-body"),
        ("PUT", "/method?kind=put", "put-body"),
        ("DELETE", "/method?kind=delete", ""),
        ("QUERY", "/method?kind=query&cursor=next", "query-body"),
    ];
    for (method, path_and_query, body) in cases {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin_address = origin.local_addr().unwrap();
        let expected_start = format!("{method} {path_and_query} HTTP/1.1\r\n");
        let expected_body = body.as_bytes().to_vec();
        let origin_task = tokio::spawn(async move {
            let (mut stream, _) = origin.accept().await.unwrap();
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
                headers.starts_with(&expected_start),
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
                expected_body
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        });
        let service =
            ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication)).unwrap();
        let (client, proxy) = tokio::io::duplex(16 * 1024);
        let proxy_task = tokio::spawn(async move {
            service
                .serve_connection(
                    Box::new(proxy),
                    "127.0.0.1:45010".parse().unwrap(),
                    CancellationToken::new(),
                )
                .await
        });
        let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
        let connection_task = tokio::spawn(connection);
        let response = sender
            .send_request(
                Request::builder()
                    .method(Method::from_bytes(method.as_bytes()).unwrap())
                    .uri(format!("http://{origin_address}{path_and_query}"))
                    .body(Full::new(Bytes::copy_from_slice(body.as_bytes())))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        drop(sender);
        connection_task.await.unwrap().unwrap();
        proxy_task.await.unwrap().unwrap();
        origin_task.await.unwrap();
    }
}

#[tokio::test]
async fn absolute_form_enters_capture_and_rule_pipeline() {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_task = tokio::spawn(async move {
        let (stream, _) = origin.accept().await.unwrap();
        let handler = service_fn(|request: Request<Incoming>| async move {
            assert_eq!(request.uri(), "/pipeline");
            assert_eq!(request.headers()["x-rule"], "applied");
            assert_eq!(
                request.into_body().collect().await.unwrap().to_bytes(),
                "rule-request"
            );
            Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(
                b"origin-response",
            ))))
        });
        server_http1::Builder::new()
            .serve_connection(TokioIo::new(stream), handler)
            .await
            .unwrap();
    });
    let ports = Arc::new(CapturingPipelinePorts {
        mutate: true,
        ..Default::default()
    });
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_pipeline(
            ChannelId::new("forward-test").unwrap(),
            Uuid::new_v4(),
            ports.clone(),
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(32 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45003".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection);
    let absolute_uri = format!("http://{origin_address}/pipeline");
    let response = sender
        .send_request(
            Request::builder()
                .method(Method::POST)
                .uri(&absolute_uri)
                .body(Full::new(Bytes::from_static(b"original-request")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "rule-response"
    );
    drop(sender);
    connection_task.await.unwrap().unwrap();
    proxy_task.await.unwrap().unwrap();
    origin_task.await.unwrap();
    let requests = ports.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].start_line,
        format!("POST {absolute_uri} HTTP/1.1")
    );
    assert_eq!(requests[0].body, "original-request");
    let responses = ports.responses.lock().unwrap();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].http_status(), Some(200));
    assert_eq!(responses[0].body, "origin-response");
}

#[tokio::test]
async fn plain_drop_without_upstream_read_closes_after_complete_request_write() {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let (request_written, request_written_rx) = oneshot::channel();
    let (release_headers, release_headers_rx) = oneshot::channel();
    let origin_task = tokio::spawn(async move {
        let (mut stream, _) = origin.accept().await.unwrap();
        assert_eq!(
            read_raw_http_request_body(&mut stream).await,
            Bytes::from_static(b"complete-request")
        );
        request_written.send(()).unwrap();
        let _ = release_headers_rx.await;
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .await;
    });
    let ports = Arc::new(CapturingPipelinePorts {
        request_actions: vec![FaultAction::DropResponse {
            read_upstream: false,
        }],
        ..Default::default()
    });
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_pipeline(
            ChannelId::new("plain-drop-write").unwrap(),
            Uuid::new_v4(),
            ports,
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(32 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45100".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection);
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        sender.send_request(
            Request::builder()
                .method(Method::POST)
                .uri(format!("http://{origin_address}/drop"))
                .body(Full::new(Bytes::from_static(b"complete-request")))
                .unwrap(),
        ),
    )
    .await
    .expect("drop must not wait for delayed response headers");
    if let Ok(response) = result {
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        panic!("drop returned {status}: {}", String::from_utf8_lossy(&body));
    }
    request_written_rx
        .await
        .expect("origin must receive the complete request first");
    let _ = release_headers.send(());
    drop(sender);
    let _ = connection_task.await;
    assert!(proxy_task.await.unwrap().is_err());
    origin_task.await.unwrap();
}

#[tokio::test]
async fn plain_drop_with_upstream_read_waits_for_segmented_response_body() {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let (first_segment, first_segment_rx) = oneshot::channel();
    let (release_tail, release_tail_rx) = oneshot::channel();
    let origin_task = tokio::spawn(async move {
        let (mut stream, _) = origin.accept().await.unwrap();
        assert_eq!(
            read_raw_http_request_body(&mut stream).await,
            Bytes::from_static(b"request")
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nabc")
            .await
            .unwrap();
        first_segment.send(()).unwrap();
        release_tail_rx.await.unwrap();
        stream.write_all(b"def").await.unwrap();
    });
    let ports = Arc::new(CapturingPipelinePorts {
        request_actions: vec![FaultAction::DropResponse {
            read_upstream: true,
        }],
        ..Default::default()
    });
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_pipeline(
            ChannelId::new("plain-drop-read").unwrap(),
            Uuid::new_v4(),
            ports,
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(32 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45101".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection);
    let request = sender.send_request(
        Request::builder()
            .method(Method::POST)
            .uri(format!("http://{origin_address}/drop"))
            .body(Full::new(Bytes::from_static(b"request")))
            .unwrap(),
    );
    tokio::pin!(request);
    first_segment_rx.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut request)
            .await
            .is_err(),
        "drop must wait for the complete upstream response body"
    );
    release_tail.send(()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1), request)
        .await
        .expect("drop must close after the final response segment");
    assert!(
        result.is_err(),
        "drop must close instead of returning a 502"
    );
    drop(sender);
    let _ = connection_task.await;
    assert!(proxy_task.await.unwrap().is_err());
    origin_task.await.unwrap();
}
