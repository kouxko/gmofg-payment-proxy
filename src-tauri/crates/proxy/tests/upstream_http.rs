//! `PROXY-006..010` / `TEST-PROXY` 的原始上游 HTTP/1.1 集成测试。
//!
//! 临时 TCP Server 会记录 Proxy 实际写出的请求并返回手工响应，从而验证请求目标、
//! Header、Body 与连接关闭语义。该层不进行 TLS，也不解释 Payment 业务 JSON。

use std::{
    net::SocketAddr,
    sync::Mutex,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Uri};
use intercept_proxy_runtime::message::{Message, MessageLimits};
use intercept_proxy_runtime::transport::{
    ConnectionContext, ForwardRequest, HandshakePolicy, HyperUpstreamConnector, NoopPipelinePorts,
    PipelinePorts, UpstreamConnector, UpstreamSecurityEvidence, UpstreamTransportSecurity,
};
use intercept_proxy_runtime::{ChannelId, FaultAction, TrafficDirection};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn test_context(peer_addr: SocketAddr) -> ConnectionContext {
    ConnectionContext {
        runtime_epoch: Uuid::new_v4(),
        connection_id: Uuid::new_v4(),
        channel: ChannelId::new("test").unwrap(),
        peer_addr,
        accepted_at: SystemTime::now(),
        tls_peer: None,
    }
}

#[derive(Debug, Default)]
struct SecurityPorts {
    evidence: Mutex<Vec<UpstreamSecurityEvidence>>,
}

impl HandshakePolicy for SecurityPorts {}

#[async_trait]
impl PipelinePorts for SecurityPorts {
    async fn upstream_security_established(
        &self,
        _context: &ConnectionContext,
        evidence: &UpstreamSecurityEvidence,
    ) {
        self.evidence.lock().unwrap().push(evidence.clone());
    }
}

async fn serve_fidelity_upstream(listener: TcpListener) {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let read = stream.read(&mut buffer).await.unwrap();
        assert_ne!(read, 0);
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") && request.ends_with(b"raw") {
            break;
        }
    }
    assert!(request.starts_with(b"POST /resource HTTP/1.1\r\n"));
    for expected in [
        b"Host: upstream.test".as_slice(),
        b"Connection: close".as_slice(),
        b"Content-Length: 3".as_slice(),
    ] {
        assert!(
            request
                .windows(expected.len())
                .any(|window| window == expected)
        );
    }
    let exact_custom_headers = b"X-Trace: first\x80\r\n\
x-Other: middle\xff\r\n\
x-TRACE: second\r\n\
x-Other: last";
    assert!(
        request
            .windows(exact_custom_headers.len())
            .any(|window| window == exact_custom_headers),
        "normal upstream path must not rebuild canonical headers from HeaderMap"
    );
    stream
        .write_all(
            b"HTTP/1.1 302 Vendor Redirect Result\r\n\
Location: http://invalid.test/\r\n\
X-Trace: first\x80\r\n\
x-Other: middle\xff\r\n\
x-TRACE: second\r\n\
x-Other: last\r\n\
Content-Length: 3\r\n\
Connection: close\r\n\r\nsjis",
        )
        .await
        .unwrap();
    stream.shutdown().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "connector must not follow redirects or retry"
    );
}

#[tokio::test]
async fn upstream_is_http11_single_use_host_rewritten_and_redirect_not_followed() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(serve_fidelity_upstream(listener));

    let connector = HyperUpstreamConnector {
        address,
        host: "upstream.test".into(),
        host_header: "upstream.test".into(),
        rewrite_host: true,
        tls: None,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        limits: MessageLimits::default(),
    };
    let mut headers = HeaderMap::new();
    headers.insert("host", HeaderValue::from_static("alpha-client"));
    headers.insert("content-length", HeaderValue::from_static("3"));
    let mut message = Message::request(
        &Method::POST,
        &Uri::from_static("/resource"),
        &headers,
        Bytes::from_static(b"raw"),
    );
    message.headers.extend(
        [
            (b"X-Trace".as_slice(), b"first\x80".as_slice()),
            (b"x-Other".as_slice(), b"middle\xff".as_slice()),
            (b"x-TRACE".as_slice(), b"second".as_slice()),
            (b"x-Other".as_slice(), b"last".as_slice()),
        ]
        .into_iter()
        .map(|(name, value)| {
            intercept_proxy_runtime::message::RawHeader::new(
                Bytes::copy_from_slice(name),
                Bytes::copy_from_slice(value),
            )
        }),
    );
    let request = ForwardRequest {
        method: Method::POST,
        uri: Uri::from_static("/resource"),
        message,
    };
    let ports = SecurityPorts::default();
    let response = connector
        .send(
            &test_context(address),
            &ports,
            request,
            &[],
            None,
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .final_response;
    {
        let evidence = ports.evidence.lock().unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            evidence[0].transport,
            UpstreamTransportSecurity::PlaintextHttp
        );
        assert_eq!(evidence[0].resolved_address, address);
        assert!(evidence[0].tls_version.is_none());
    }
    assert_eq!(response.start_line, "HTTP/1.1 302 Vendor Redirect Result");
    assert_eq!(
        response
            .headers
            .iter()
            .filter(|header| {
                header.name.eq_ignore_ascii_case(b"x-trace")
                    || header.name.eq_ignore_ascii_case(b"x-other")
            })
            .map(|header| (header.name.as_ref(), header.value.as_ref()))
            .collect::<Vec<_>>(),
        vec![
            (b"X-Trace".as_slice(), b"first\x80".as_slice()),
            (b"x-Other".as_slice(), b"middle\xff".as_slice()),
            (b"x-TRACE".as_slice(), b"second".as_slice()),
            (b"x-Other".as_slice(), b"last".as_slice()),
        ]
    );
    assert_eq!(response.body, Bytes::from_static(b"sji"));
    server.await.unwrap();
}

#[tokio::test]
async fn upstream_informational_heads_do_not_replace_the_final_raw_response_head() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(
                b"HTTP/1.1 100 Continue\r\nX-Info:\t first \r\n\r\n\
HTTP/1.1 103 Early Hints\r\nLink: </style.css>\r\n\r\n\
HTTP/1.1 207 Product Final\r\nX-Final:\t  yes \t\r\n\
Content-Length: 2\r\nConnection: close\r\n\r\nOK",
            )
            .await
            .unwrap();
    });
    let connector = fault_test_connector(address);

    let exchange = connector
        .send(
            &test_context(address),
            &NoopPipelinePorts,
            fault_test_request(),
            &[],
            None,
            &CancellationToken::new(),
        )
        .await
        .expect("final response");
    assert_eq!(
        exchange.informational_heads,
        vec![
            Bytes::from_static(b"HTTP/1.1 100 Continue\r\nX-Info:\t first \r\n\r\n"),
            Bytes::from_static(b"HTTP/1.1 103 Early Hints\r\nLink: </style.css>\r\n\r\n"),
        ]
    );
    let response = exchange.final_response;

    assert_eq!(response.start_line, "HTTP/1.1 207 Product Final");
    assert_eq!(response.http_status(), Some(207));
    assert_eq!(response.body, Bytes::from_static(b"OK"));
    assert!(
        response
            .reconstruct()
            .starts_with(b"HTTP/1.1 207 Product Final\r\nX-Final:\t  yes \t\r\n")
    );
    server.await.unwrap();
}

#[tokio::test]
async fn oversized_upstream_body_is_classified() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n12345")
            .await
            .unwrap();
    });
    let connector = HyperUpstreamConnector {
        address,
        host: "localhost".into(),
        host_header: "localhost".into(),
        rewrite_host: true,
        tls: None,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        limits: MessageLimits {
            max_body_bytes: 4,
            ..MessageLimits::default()
        },
    };
    let request = ForwardRequest {
        method: Method::GET,
        uri: Uri::from_static("/"),
        message: Message::request(
            &Method::GET,
            &Uri::from_static("/"),
            &HeaderMap::new(),
            Bytes::new(),
        ),
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
        .unwrap_err();
    assert_eq!(error.code, "BODY_TOO_LARGE");
    server.await.unwrap();
}

fn fault_test_connector(address: std::net::SocketAddr) -> HyperUpstreamConnector {
    HyperUpstreamConnector {
        address,
        host: "upstream.test".into(),
        host_header: "upstream.test".into(),
        rewrite_host: true,
        tls: None,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(2),
        limits: MessageLimits::default(),
    }
}

fn fault_test_request() -> ForwardRequest {
    let mut headers = HeaderMap::new();
    headers.insert("host", HeaderValue::from_static("alpha-client"));
    headers.insert("content-length", HeaderValue::from_static("3"));
    ForwardRequest {
        method: Method::POST,
        uri: Uri::from_static("/resource"),
        message: Message::request(
            &Method::POST,
            &Uri::from_static("/resource"),
            &headers,
            Bytes::from_static(b"raw"),
        ),
    }
}

async fn accept_request_until_eof(listener: TcpListener) -> Vec<u8> {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    stream.read_to_end(&mut request).await.unwrap();
    request
}

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

#[tokio::test]
async fn backpressured_request_body_uses_write_timeout_and_releases_connection() {
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
        .expect_err("an upstream that does not read must hit the write-stage timeout");
    assert_eq!(error.code, "UPSTREAM_WRITE_TIMEOUT");
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
