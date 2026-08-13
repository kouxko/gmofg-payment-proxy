#[tokio::test]
async fn backpressured_request_body_timeout_releases_connection() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut received = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut received))
            .await
            .expect("timed-out client must release its upstream connection")
            .ok();
    });

    let body = Bytes::from(vec![b'x'; 32 * 1024 * 1024]);
    let mut headers = HeaderMap::new();
    headers.insert("host", HeaderValue::from_static("alpha-client"));
    headers.insert(
        "content-length",
        HeaderValue::from_str(&body.len().to_string()).unwrap(),
    );
    let request = ForwardRequest {
        method: Method::POST,
        uri: Uri::from_static("/resource"),
        message: Message::request(
            &Method::POST,
            &Uri::from_static("/resource"),
            &headers,
            body,
        ),
    };
    let connector = HyperUpstreamConnector {
        write_timeout: Duration::from_millis(20),
        read_timeout: Duration::from_secs(2),
        limits: MessageLimits {
            max_body_bytes: 32 * 1024 * 1024,
            ..MessageLimits::default()
        },
        ..fault_test_connector(address)
    };

    let error = connector
        .send(
            &test_context(address),
            &NoopPipelinePorts,
            request,
            &[],
            None,
            &CancellationToken::new(),
        )
        .await
        .expect_err("an upstream that does not read must time out");
    assert!(
        matches!(
            error.code,
            "UPSTREAM_WRITE_TIMEOUT" | "UPSTREAM_READ_TIMEOUT"
        ),
        "the OS may buffer the complete request before the timeout; got {error:?}"
    );
    server.await.unwrap();
}

// WN-009, ACTION-017, TEST-WEAK-NETWORK:
// the HTTP/1.1 connector must expose a stable intentional-abort result after
// writing exactly the configured request-body prefix.
#[tokio::test]
async fn upstream_mid_body_disconnect_writes_exact_prefix_and_is_classified() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(accept_request_until_eof(listener));
    let connector = fault_test_connector(address);
    let body = Bytes::from_static(b"abcdefgh");
    let mut headers = HeaderMap::new();
    headers.insert("host", HeaderValue::from_static("alpha-client"));
    headers.insert("content-length", HeaderValue::from_static("8"));
    let request = ForwardRequest {
        method: Method::POST,
        uri: Uri::from_static("/resource"),
        message: Message::request(
            &Method::POST,
            &Uri::from_static("/resource"),
            &headers,
            body,
        ),
    };

    let error = connector
        .send(
            &test_context(address),
            &NoopPipelinePorts,
            request,
            &[FaultAction::DisconnectDuringWrite {
                after_bytes: 3,
                direction: TrafficDirection::Upstream,
            }],
            None,
            &CancellationToken::new(),
        )
        .await
        .expect_err("intentional upstream body abort");
    assert_eq!(error.code, "FAULT_STREAM_ABORTED");

    let request = server.await.unwrap();
    assert!(
        request
            .windows(b"Content-Length: 8".len())
            .any(|window| window == b"Content-Length: 8"),
        "{request:?}"
    );
    assert!(request.ends_with(b"\r\n\r\nabc"), "{request:?}");
}
