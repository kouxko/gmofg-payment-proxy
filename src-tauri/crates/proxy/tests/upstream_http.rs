//! Raw upstream HTTP/1.1 integration coverage for PROXY-006..010 / TEST-PROXY.

use std::time::Duration;

use bytes::Bytes;
use gmofg_proxy_runtime::message::{Message, MessageLimits};
use gmofg_proxy_runtime::transport::{ForwardRequest, HyperUpstreamConnector, UpstreamConnector};
use http::{HeaderMap, HeaderValue, Method, Uri};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn upstream_is_http11_single_use_host_rewritten_and_redirect_not_followed() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
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
        assert!(request.starts_with(b"POST /payment HTTP/1.1\r\n"));
        assert!(
            request
                .windows(b"host: upstream.test".len())
                .any(|window| window.eq_ignore_ascii_case(b"host: upstream.test"))
        );
        assert!(
            request
                .windows(b"connection: close".len())
                .any(|window| window.eq_ignore_ascii_case(b"connection: close"))
        );
        stream
            .write_all(
                b"HTTP/1.1 302 Found\r\nLocation: http://invalid.test/\r\nContent-Length: 3\r\nConnection: close\r\n\r\nsjis",
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
    });

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
    headers.insert("host", HeaderValue::from_static("payment-app"));
    headers.insert("content-length", HeaderValue::from_static("3"));
    let request = ForwardRequest {
        method: Method::POST,
        uri: Uri::from_static("/payment"),
        message: Message::request(
            &Method::POST,
            &Uri::from_static("/payment"),
            &headers,
            Bytes::from_static(b"raw"),
        ),
    };
    let response = connector
        .send(request, &[], &CancellationToken::new())
        .await
        .unwrap();
    assert!(response.start_line.starts_with("HTTP/1.1 302"));
    assert_eq!(response.body, Bytes::from_static(b"sji"));
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
        .send(request, &[], &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "BODY_TOO_LARGE");
    server.await.unwrap();
}
