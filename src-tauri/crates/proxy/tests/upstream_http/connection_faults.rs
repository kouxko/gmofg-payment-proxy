#[tokio::test]
async fn close_after_request_write_sends_the_complete_request_without_waiting_for_response() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(accept_request_until_eof(listener));
    let connector = fault_test_connector(address);
    let mut request = fault_test_request();
    request.message.remove_header("content-length");
    request
        .message
        .headers
        .push(intercept_proxy_runtime::message::RawHeader::new(
            Bytes::from_static(b"transfer-encoding"),
            Bytes::from_static(b"chunked"),
        ));

    let error = tokio::time::timeout(
        Duration::from_millis(500),
        connector.send(
            &test_context(address),
            &NoopPipelinePorts,
            request,
            &[FaultAction::DropResponse {
                read_upstream: false,
            }],
            None,
            &CancellationToken::new(),
        ),
    )
    .await
    .expect("close-after-write must not wait for response headers")
    .expect_err("close-after-write terminates the upstream exchange");
    assert_eq!(error.code, "BREAKPOINT_CLIENT_DISCONNECTED");
    assert_eq!(
        error.message,
        "upstream request intentionally closed after complete write"
    );

    let request = server.await.unwrap();
    assert!(request.starts_with(b"POST /resource HTTP/1.1\r\n"));
    assert!(
        request
            .windows(b"Host: upstream.test".len())
            .any(|window| window == b"Host: upstream.test")
    );
    assert!(
        request
            .windows(b"Content-Length: 3".len())
            .any(|window| window == b"Content-Length: 3")
    );
    assert!(
        !request
            .windows(b"Transfer-Encoding".len())
            .any(|window| window == b"Transfer-Encoding")
    );
    assert!(request.ends_with(b"\r\n\r\nraw"));
}

#[tokio::test]
async fn injected_read_timeout_starts_after_the_complete_request_write() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(accept_request_until_eof(listener));
    let connector = fault_test_connector(address);

    let error = tokio::time::timeout(
        Duration::from_millis(500),
        connector.send(
            &test_context(address),
            &NoopPipelinePorts,
            fault_test_request(),
            &[FaultAction::UpstreamReadTimeout(Duration::from_millis(40))],
            None,
            &CancellationToken::new(),
        ),
    )
    .await
    .expect("injected read timeout must not wait for global read timeout or response headers")
    .expect_err("injected read timeout");
    assert_eq!(error.code, "UPSTREAM_READ_TIMEOUT");
    assert_eq!(error.message, "injected timeout after 40 ms");

    let request = server.await.unwrap();
    assert!(request.starts_with(b"POST /resource HTTP/1.1\r\n"));
    assert!(
        request
            .windows(b"Host: upstream.test".len())
            .any(|window| window == b"Host: upstream.test")
    );
    assert!(
        request
            .windows(b"Content-Length: 3".len())
            .any(|window| window == b"Content-Length: 3")
    );
    assert!(request.ends_with(b"\r\n\r\nraw"));
}
