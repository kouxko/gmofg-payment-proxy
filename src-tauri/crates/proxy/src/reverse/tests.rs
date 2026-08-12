use super::*;

#[test]
fn reverse_identity_debug_redacts_private_key_material() {
    let identity = ReverseClientIdentity {
        certificate_chain_der: vec![vec![1, 2, 3]],
        private_key_pkcs8_der: Zeroizing::new(b"unique-private-key-material".to_vec()),
    };

    let debug = format!("{identity:?}");
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("unique-private-key-material"));
    assert!(!debug.contains("117, 110, 105, 113"));
}

#[tokio::test]
async fn upstream_host_header_omits_default_ports_and_brackets_ipv6() {
    let cases = [
        ("http://127.0.0.1", "127.0.0.1"),
        ("https://127.0.0.1", "127.0.0.1"),
        ("http://127.0.0.1:8080", "127.0.0.1:8080"),
        ("http://[::1]", "[::1]"),
        ("https://[::1]", "[::1]"),
        ("https://[::1]:8443", "[::1]:8443"),
    ];

    for (origin, expected) in cases {
        let endpoint = UpstreamEndpoint::parse(origin, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(endpoint.host_header, expected, "origin: {origin}");
    }
}

#[tokio::test]
async fn plaintext_reverse_listener_preserves_every_byte() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let expected = b"POST /x HTTP/1.1\r\nHost: preserved.test\r\nX-Odd:  a  b\r\nContent-Length: 5\r\n\r\n\x00\x81abc".to_vec();
    let expected_for_server = expected.clone();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut actual = vec![0; expected_for_server.len()];
        stream.read_exact(&mut actual).await.unwrap();
        assert_eq!(actual, expected_for_server);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\x00\x81ok")
            .await
            .unwrap();
    });
    let downstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let downstream_address = downstream.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: downstream_address,
        allowed_client_cidrs: Vec::new(),
        upstream_origin: format!("http://{upstream_address}"),
        downstream_tls: None,
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();
    let serve_cancellation = cancellation.clone();
    let serve =
        tokio::spawn(async move { service.serve_listener(downstream, serve_cancellation).await });
    let mut client = TcpStream::connect(downstream_address).await.unwrap();
    client.write_all(&expected).await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(
        response,
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\x00\x81ok"
    );
    upstream_task.await.unwrap();
    cancellation.cancel();
    serve.await.unwrap().unwrap();
}

#[tokio::test]
async fn raw_relay_preserves_binary_payload_larger_than_its_buffer() {
    let payload = (0..(32 * 1024 + 73))
        .map(|index| u8::try_from(index % 251).unwrap())
        .collect::<Vec<_>>();
    let expected = payload.clone();
    let (mut downstream, relay_downstream) = tokio::io::duplex(64 * 1024);
    let (relay_upstream, mut upstream) = tokio::io::duplex(64 * 1024);
    let relay = tokio::spawn(relay_exact(
        Box::new(relay_downstream),
        Box::new(relay_upstream),
        Duration::from_secs(1),
        Duration::from_secs(1),
        CancellationToken::new(),
    ));

    downstream.write_all(&payload).await.unwrap();
    downstream.shutdown().await.unwrap();
    let mut actual = Vec::new();
    upstream.read_to_end(&mut actual).await.unwrap();

    assert_eq!(actual, expected);
    drop(downstream);
    drop(upstream);
    relay.await.unwrap().unwrap();
}

#[tokio::test]
async fn raw_relay_propagates_downstream_half_close_before_upstream_reply() {
    let (mut downstream, relay_downstream) = tokio::io::duplex(1024);
    let (relay_upstream, mut upstream) = tokio::io::duplex(1024);
    let relay = tokio::spawn(relay_exact(
        Box::new(relay_downstream),
        Box::new(relay_upstream),
        Duration::from_secs(1),
        Duration::from_secs(1),
        CancellationToken::new(),
    ));
    let upstream_task = tokio::spawn(async move {
        let mut request = Vec::new();
        upstream.read_to_end(&mut request).await.unwrap();
        assert_eq!(request, b"request-before-half-close");
        upstream.write_all(b"reply-after-half-close").await.unwrap();
        upstream.shutdown().await.unwrap();
    });

    downstream
        .write_all(b"request-before-half-close")
        .await
        .unwrap();
    downstream.shutdown().await.unwrap();
    let mut reply = Vec::new();
    downstream.read_to_end(&mut reply).await.unwrap();

    assert_eq!(reply, b"reply-after-half-close");
    upstream_task.await.unwrap();
    relay.await.unwrap().unwrap();
}

#[tokio::test]
async fn raw_relay_reports_read_timeout_for_silent_peers() {
    let (_downstream, relay_downstream) = tokio::io::duplex(64);
    let (relay_upstream, _upstream) = tokio::io::duplex(64);

    let error = relay_exact(
        Box::new(relay_downstream),
        Box::new(relay_upstream),
        Duration::from_millis(10),
        Duration::from_secs(1),
        CancellationToken::new(),
    )
    .await
    .expect_err("silent raw relay must reach its read timeout");

    assert_eq!(error.code, ErrorCode::UpstreamReadTimeout.as_str());
}

#[tokio::test]
async fn raw_relay_reports_proxy_stopped_when_cancelled() {
    let (_downstream, relay_downstream) = tokio::io::duplex(64);
    let (relay_upstream, _upstream) = tokio::io::duplex(64);
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = relay_exact(
        Box::new(relay_downstream),
        Box::new(relay_upstream),
        Duration::from_secs(30),
        Duration::from_secs(30),
        cancellation,
    )
    .await
    .expect_err("cancelled raw relay must stop");

    assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
}

#[tokio::test]
async fn raw_listener_stop_returns_with_an_active_silent_connection() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
    let accepted = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        accepted_tx.send(()).unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
        drop(stream);
    });
    let downstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let downstream_address = downstream.local_addr().unwrap();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: downstream_address,
        allowed_client_cidrs: Vec::new(),
        upstream_origin: format!("http://{upstream_address}"),
        downstream_tls: None,
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(30),
        write_timeout: Duration::from_secs(30),
    })
    .await
    .unwrap();
    let cancellation = CancellationToken::new();
    let stop = cancellation.clone();
    let serve = tokio::spawn(async move { service.serve_listener(downstream, cancellation).await });
    let _client = TcpStream::connect(downstream_address).await.unwrap();
    accepted_rx.await.unwrap();

    stop.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), serve)
        .await
        .expect("current raw listener stop must return")
        .unwrap();

    assert!(result.is_ok());
    accepted.abort();
}

#[tokio::test]
async fn fixed_server_build_reports_dns_failure_before_serving() {
    let error = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        allowed_client_cidrs: Vec::new(),
        upstream_origin: "http://phase-zero-does-not-exist.invalid:18080".into(),
        downstream_tls: None,
        upstream_tls: None,
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
    })
    .await
    .expect_err("fixed server build must resolve its target eagerly");

    assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str());
}

#[tokio::test]
async fn non_loopback_listener_allows_all_clients_without_cidr() {
    ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: "0.0.0.0:18080".parse().unwrap(),
        allowed_client_cidrs: Vec::new(),
        upstream_origin: "http://127.0.0.1:9".into(),
        downstream_tls: None,
        upstream_tls: None,
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
    })
    .await
    .expect("empty CIDR list explicitly allows all clients");
}
