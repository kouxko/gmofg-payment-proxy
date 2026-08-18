use super::*;

#[path = "tests/tail_write.rs"]
mod tail_write;
use std::net::{IpAddr, Ipv4Addr};

#[test]
fn response_head_capture_skips_informational_heads_and_keeps_the_final_head() {
    let mut capture = RawHttp1HeadCapture::final_response();
    capture.record(
        b"HTTP/1.1 100 Continue\r\nX-Info:\t first \r\n\r\n\
HTTP/1.1 103 Early Hints\r\nLink: </style.css>\r\n\r\n\
HTTP/1.1 299 Vendor Final\r\nX-Final: yes\r\n\r\nbody",
        1024,
    );

    assert_eq!(
        capture.required_head("response").expect("final head"),
        Bytes::from_static(b"HTTP/1.1 299 Vendor Final\r\nX-Final: yes\r\n\r\n")
    );
}

#[test]
fn raw_head_capture_reports_limit_exhaustion_instead_of_falling_back() {
    let mut capture = RawHttp1HeadCapture::default();
    capture.record(b"GET / HTTP/1.1\r\nX-Test: value\r\n\r\n", 12);

    let error = capture
        .required_head("request")
        .expect_err("truncated capture must fail closed");
    assert_eq!(error.code, ErrorCode::HeaderLimitExceeded.as_str());
}

#[derive(Debug)]
struct FixedResponseConnector {
    body: Bytes,
    declared_content_length: Option<usize>,
}

#[derive(Debug)]
struct UnusedAcceptor;

#[derive(Debug, Clone, Copy)]
enum PendingWriteStage {
    Tail,
    Flush,
    Shutdown,
}

#[derive(Debug)]
struct PendingWriteIo(PendingWriteStage);

impl AsyncRead for PendingWriteIo {
    fn poll_read(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        _buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for PendingWriteIo {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.0 {
            PendingWriteStage::Tail => Poll::Pending,
            PendingWriteStage::Flush | PendingWriteStage::Shutdown => Poll::Ready(Ok(buffer.len())),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.0 {
            PendingWriteStage::Flush => Poll::Pending,
            PendingWriteStage::Tail | PendingWriteStage::Shutdown => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.0 {
            PendingWriteStage::Tail | PendingWriteStage::Flush => Poll::Ready(Ok(())),
            PendingWriteStage::Shutdown => Poll::Pending,
        }
    }
}

#[async_trait]
impl ConnectionAcceptor for UnusedAcceptor {
    async fn accept(&self, _io: BoxIo, _context: &ConnectionContext) -> Result<AcceptedConnection> {
        unreachable!("run_connection_inner does not invoke the acceptor")
    }
}

#[async_trait]
impl UpstreamConnector for FixedResponseConnector {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        _request: ForwardRequest,
        _actions: &[FaultAction],
        _informational: Option<&InformationalResponseSink>,
        _cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
        let mut message = Message::response(StatusCode::OK, &HeaderMap::new(), self.body.clone());
        if let Some(length) = self.declared_content_length {
            message.set_content_length(length);
        }
        Ok(message.into())
    }
}

fn downstream_test_service(
    body: Bytes,
    declared_content_length: Option<usize>,
    write_timeout: Duration,
) -> ConnectionService {
    ConnectionService {
        acceptor: Arc::new(UnusedAcceptor),
        upstream: Arc::new(FixedResponseConnector {
            body,
            declared_content_length,
        }),
        ports: Arc::new(NoopPipelinePorts),
        clock: Arc::new(SystemClock),
        admission: ConnectionAdmission::new(1).expect("valid test capacity"),
        allowed_client_cidrs: Vec::new(),
        limits: MessageLimits::default(),
        read_timeout: Duration::from_secs(1),
        write_timeout,
    }
}

fn downstream_test_context() -> ConnectionContext {
    ConnectionContext {
        runtime_epoch: Uuid::new_v4(),
        connection_id: Uuid::new_v4(),
        channel: ChannelId::new("alpha").expect("valid test channel ID"),
        peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12_345),
        accepted_at: SystemTime::now(),
        tls_peer: None,
    }
}

async fn write_test_request(client: &mut tokio::io::DuplexStream) {
    client
        .write_all(b"POST / HTTP/1.1\r\nHost: proxy.test\r\nContent-Length: 0\r\n\r\n")
        .await
        .expect("write test request");
}

#[tokio::test]
async fn response_from_disposition_without_body_declares_zero_length() {
    let canonical_head = StdMutex::new(None);
    let raw_tail = StdMutex::new(None);
    let fault = StdMutex::new(None);

    let response = response_from_disposition(
        ResponseDisposition::Send {
            message: Message::response(StatusCode::OK, &HeaderMap::new(), Bytes::new()),
            schedule: TrafficSchedule::default(),
        },
        &canonical_head,
        &raw_tail,
        &fault,
        &CancellationToken::new(),
    )
    .expect("empty body response must build");

    assert_eq!(response.headers().get("content-length").unwrap(), "0");
    let mut body = response.into_body();
    assert!(body.frame().await.is_none());
    assert!(
        fault
            .lock()
            .expect("intentional fault mutex poisoned")
            .is_none()
    );
}

#[tokio::test]
async fn response_from_disposition_updates_content_length_after_body_text_change() {
    let canonical_head = StdMutex::new(None);
    let raw_tail = StdMutex::new(None);
    let fault = StdMutex::new(None);

    let response = response_from_disposition(
        ResponseDisposition::Send {
            message: Message::response(
                StatusCode::OK,
                &HeaderMap::new(),
                Bytes::from_static(b"changed-body"),
            ),
            schedule: TrafficSchedule::default(),
        },
        &canonical_head,
        &raw_tail,
        &fault,
        &CancellationToken::new(),
    )
    .expect("rewritten response should build");

    assert_eq!(
        response.headers().get("content-length").unwrap(),
        http::HeaderValue::from_static("12")
    );
    assert!(
        fault
            .lock()
            .expect("intentional fault mutex poisoned")
            .is_none()
    );
}

#[tokio::test]
async fn stalled_upstream_request_write_has_write_timeout_classification() {
    let body = Bytes::from_static(b"request body");
    let mut headers = HeaderMap::new();
    headers.insert("host", http::HeaderValue::from_static("upstream.test"));
    headers.insert(
        "content-length",
        http::HeaderValue::from_str(&body.len().to_string()).expect("valid content length"),
    );
    let uri = http::Uri::from_static("/resource");
    let request = ForwardRequest {
        method: Method::POST,
        uri: uri.clone(),
        message: Message::request(&Method::POST, &uri, &headers, body),
    };

    let error = send_http1_request(
        Box::new(PendingWriteIo(PendingWriteStage::Tail)),
        request,
        Http1ExchangeConfig {
            schedule: TrafficSchedule::default(),
            write_timeout: Duration::from_millis(5),
            read_timeout: Duration::from_secs(1),
            limits: MessageLimits::default(),
        },
        None,
        &CancellationToken::new(),
    )
    .await
    .expect_err("a stalled request write must hit the write-stage timeout");

    assert_eq!(error.code, ErrorCode::UpstreamWriteTimeout.as_str());
}

#[tokio::test]
async fn intentional_content_length_and_truncation_faults_have_stable_classifications() {
    let cases = [
        (
            ResponseDisposition::Send {
                message: {
                    let mut message = Message::response(
                        StatusCode::OK,
                        &HeaderMap::new(),
                        Bytes::from_static(b"body"),
                    );
                    message.set_content_length(24);
                    message
                },
                schedule: TrafficSchedule::default(),
            },
            IntentionalWireFault::IncorrectContentLength,
            ErrorCode::IncorrectContentLength,
        ),
        (
            ResponseDisposition::Send {
                message: {
                    let mut message = Message::response(
                        StatusCode::OK,
                        &HeaderMap::new(),
                        Bytes::from_static(b"body"),
                    );
                    message.set_content_length(2);
                    message
                },
                schedule: TrafficSchedule::default(),
            },
            IntentionalWireFault::IncorrectContentLength,
            ErrorCode::IncorrectContentLength,
        ),
        (
            ResponseDisposition::Truncate {
                message: Message::response(
                    StatusCode::OK,
                    &HeaderMap::new(),
                    Bytes::from_static(b"body"),
                ),
                bytes: 2,
                schedule: TrafficSchedule::default(),
            },
            IntentionalWireFault::TruncatedResponse,
            ErrorCode::TruncatedResponse,
        ),
    ];

    for (disposition, expected_fault, expected_code) in cases {
        let canonical_head = StdMutex::new(None);
        let raw_tail = StdMutex::new(None);
        let fault = StdMutex::new(None);
        response_from_disposition(
            disposition,
            &canonical_head,
            &raw_tail,
            &fault,
            &CancellationToken::new(),
        )
        .expect("intentional wire response should be constructed");
        let actual = fault
            .lock()
            .expect("intentional wire fault mutex poisoned")
            .expect("intentional wire fault marker");
        assert_eq!(actual, expected_fault);
        assert_eq!(actual.error().code, expected_code.as_str());
    }
}

#[tokio::test]
async fn downstream_mid_body_disconnect_sends_exact_prefix_and_keeps_declared_length() {
    let canonical_head = StdMutex::new(None);
    let raw_tail = StdMutex::new(None);
    let fault = StdMutex::new(None);
    let response = response_from_disposition(
        ResponseDisposition::Send {
            message: Message::response(
                StatusCode::OK,
                &HeaderMap::new(),
                Bytes::from_static(b"abcdefgh"),
            ),
            schedule: TrafficSchedule {
                disconnect_after_bytes: Some(3),
                ..TrafficSchedule::default()
            },
        },
        &canonical_head,
        &raw_tail,
        &fault,
        &CancellationToken::new(),
    )
    .expect("downstream abort response");
    assert_eq!(
        response.headers().get("content-length").unwrap(),
        http::HeaderValue::from_static("8")
    );
    let mut body = response.into_body();
    let prefix = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(prefix, Bytes::from_static(b"abc"));
    assert!(body.frame().await.is_none());
    assert_eq!(
        *fault.lock().expect("intentional fault mutex poisoned"),
        Some(IntentionalWireFault::StreamAborted)
    );
}

// ACTION-003~005, TEST-FAULT:
// the runtime must wait for each rule's exact duration, not the global connector timeout.
#[tokio::test]
async fn injected_timeouts_use_the_duration_carried_by_each_rule_action() {
    let cases = [
        (
            InjectedTimeoutStage::Connect,
            FaultAction::UpstreamConnectTimeout(Duration::from_millis(2)),
            ErrorCode::UpstreamConnectTimeout,
            2,
        ),
        (
            InjectedTimeoutStage::Write,
            FaultAction::UpstreamWriteTimeout(Duration::from_millis(3)),
            ErrorCode::UpstreamWriteTimeout,
            3,
        ),
        (
            InjectedTimeoutStage::Read,
            FaultAction::UpstreamReadTimeout(Duration::from_millis(4)),
            ErrorCode::UpstreamReadTimeout,
            4,
        ),
    ];

    for (stage, action, code, milliseconds) in cases {
        let error = wait_for_injected_timeout(&[action], stage, &CancellationToken::new())
            .await
            .expect_err("configured timeout must terminate with its stage error");
        assert_eq!(error.code, code.as_str());
        assert_eq!(
            error.message,
            format!("injected timeout after {milliseconds} ms")
        );
    }
}

#[tokio::test]
async fn injected_timeout_only_applies_to_its_matching_stage() {
    let action = FaultAction::UpstreamReadTimeout(Duration::from_mins(1));
    wait_for_injected_timeout(
        &[action],
        InjectedTimeoutStage::Connect,
        &CancellationToken::new(),
    )
    .await
    .expect("read timeout must not affect connect stage");
}

#[tokio::test]
async fn injected_timeouts_stop_immediately_when_proxy_is_cancelled() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    for (stage, action) in [
        (
            InjectedTimeoutStage::Connect,
            FaultAction::UpstreamConnectTimeout(Duration::from_mins(1)),
        ),
        (
            InjectedTimeoutStage::Write,
            FaultAction::UpstreamWriteTimeout(Duration::from_mins(1)),
        ),
        (
            InjectedTimeoutStage::Read,
            FaultAction::UpstreamReadTimeout(Duration::from_mins(1)),
        ),
    ] {
        let error = wait_for_injected_timeout(&[action], stage, &cancellation)
            .await
            .expect_err("proxy stop must cancel every injected timeout");
        assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
    }
}

#[tokio::test]
async fn downstream_response_write_respects_write_timeout() {
    let (mut client, server) = tokio::io::duplex(128);
    write_test_request(&mut client).await;
    let service = downstream_test_service(
        Bytes::from(vec![b'x'; 4 * 1024]),
        None,
        Duration::from_millis(10),
    );

    let error = tokio::time::timeout(
        Duration::from_secs(1),
        service.run_connection_inner(
            Box::new(server),
            &downstream_test_context(),
            CancellationToken::new(),
        ),
    )
    .await
    .expect("downstream write must terminate within the configured timeout")
    .expect_err("a downstream client that does not read must time out");

    assert_eq!(error.code, ErrorCode::Io.as_str());
    assert!(error.message.contains("timed out after 10 ms"));
}

#[tokio::test]
async fn downstream_response_write_stops_when_supervisor_cancels() {
    let (mut client, server) = tokio::io::duplex(128);
    write_test_request(&mut client).await;
    let service = downstream_test_service(
        Bytes::from(vec![b'x'; 4 * 1024]),
        None,
        Duration::from_secs(30),
    );
    let cancellation = CancellationToken::new();
    let stop = cancellation.clone();
    let context = downstream_test_context();

    let ((), result) = tokio::join!(
        async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            stop.cancel();
        },
        service.run_connection_inner(Box::new(server), &context, cancellation,)
    );
    let error = result.expect_err("supervisor cancellation must stop the downstream write");

    assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
}
