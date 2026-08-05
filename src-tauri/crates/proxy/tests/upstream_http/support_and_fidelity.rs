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
