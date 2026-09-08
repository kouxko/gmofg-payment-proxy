use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode};

use super::super::{HttpExchangeOutput, HttpExchangeRequest};
use super::*;
use crate::http::{ChannelId, HandshakePolicy, NoopPipelinePorts, SystemClock, UpstreamExchange};
use tokio::sync::mpsc;

#[derive(Debug, Default)]
struct RecordingWirePolicy {
    requests: AtomicUsize,
    responses: AtomicUsize,
    request_metadata: Mutex<Vec<crate::http::HttpRequestMetadata>>,
}

impl HandshakePolicy for RecordingWirePolicy {}

#[async_trait]
impl PipelinePorts for RecordingWirePolicy {
    async fn apply_request_policy(
        &self,
        _context: &ConnectionContext,
        request: &crate::http::HttpRequestMetadata,
        message: &mut Message,
    ) -> super::super::Result<Vec<FaultAction>> {
        self.request_metadata
            .lock()
            .expect("request metadata mutex poisoned")
            .push(request.clone());
        self.requests.fetch_add(1, Ordering::SeqCst);
        message.replace_body(Bytes::from_static(b"policy-request"));
        Ok(Vec::new())
    }

    async fn apply_response_policy(
        &self,
        _context: &ConnectionContext,
        request: &crate::http::HttpRequestMetadata,
        message: &mut Message,
    ) -> super::super::Result<Vec<FaultAction>> {
        self.request_metadata
            .lock()
            .expect("request metadata mutex poisoned")
            .push(request.clone());
        self.responses.fetch_add(1, Ordering::SeqCst);
        message.replace_body(Bytes::from_static(b"policy-response"));
        Ok(vec![FaultAction::CustomStatus(
            StatusCode::from_u16(209).unwrap(),
        )])
    }
}

#[derive(Debug, Default)]
struct RecordingConnector {
    bodies: Mutex<Vec<Bytes>>,
}

#[derive(Debug)]
struct ControlledConnector {
    confirm_flush: bool,
    started: Arc<tokio::sync::Notify>,
    release_response: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl UpstreamConnector for ControlledConnector {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        _request: ForwardRequest,
        _actions: &[FaultAction],
        write_receipt: crate::http::UpstreamWriteReceipt,
        _informational: Option<&InformationalResponseSink>,
        _cancellation: &CancellationToken,
    ) -> super::super::Result<UpstreamExchange> {
        if self.confirm_flush {
            write_receipt.confirm_flushed();
        }
        self.started.notify_one();
        if self.confirm_flush {
            self.release_response.notified().await;
        }
        Ok(Message::response(
            StatusCode::OK,
            &HeaderMap::new(),
            Bytes::from_static(b"actor-response"),
        )
        .into())
    }
}

#[async_trait]
impl UpstreamConnector for RecordingConnector {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        request: ForwardRequest,
        _actions: &[FaultAction],
        write_receipt: crate::http::UpstreamWriteReceipt,
        _informational: Option<&InformationalResponseSink>,
        _cancellation: &CancellationToken,
    ) -> super::super::Result<UpstreamExchange> {
        write_receipt.confirm_flushed();
        self.bodies
            .lock()
            .expect("connector body mutex poisoned")
            .push(request.message.body);
        Ok(Message::response(
            StatusCode::OK,
            &HeaderMap::new(),
            Bytes::from_static(b"origin-response"),
        )
        .into())
    }
}

#[tokio::test]
async fn request_reader_preserves_original_and_writer_reports_effective_wire_message() {
    let policy = Arc::new(RecordingWirePolicy::default());
    let state = state(policy.clone());
    let (sender, receiver) = mpsc::channel(1);
    let mut app = BufferedApp::new(Arc::clone(&state), receiver, "server:80".into());
    let (completed, _output) = tokio::sync::oneshot::channel();
    sender
        .send(HttpExchangeInput::Request(command(
            "server:80",
            request("app-request"),
            completed,
        )))
        .await
        .unwrap();

    let read = app.reader().read().await.unwrap().unwrap();
    assert_eq!(read.body, "app-request");

    let connector = Arc::new(RecordingConnector::default());
    {
        let mut state = state.lock().unwrap();
        let transaction = state.current.as_mut().unwrap();
        transaction
            .request
            .as_mut()
            .unwrap()
            .replace_body(Bytes::from_static(b"policy-request"));
        transaction.request_actions = Some(Vec::new());
    }
    let mut writer = server_writer(Arc::clone(&state), policy.clone(), connector.clone());
    let written = writer
        .write(HttpContext {
            header: read.header,
            body: "app-request".into(),
            body_is_utf8: true,
            wire_body: b"app-request".to_vec(),
        })
        .await
        .unwrap()
        .into_context();

    assert_eq!(policy.requests.load(Ordering::SeqCst), 0);
    assert_eq!(
        connector
            .bodies
            .lock()
            .expect("connector body mutex poisoned")
            .as_slice(),
        &[Bytes::from_static(b"policy-request")]
    );
    assert_eq!(written.body, "policy-request");
}

#[tokio::test]
async fn response_reader_preserves_original_and_writer_reports_effective_wire_message() {
    let policy = Arc::new(RecordingWirePolicy::default());
    let state = state(policy.clone());
    let (completed, output) = tokio::sync::oneshot::channel();
    state
        .lock()
        .expect("HTTP Exchange state mutex poisoned")
        .begin(command("server:80", request("request"), completed))
        .unwrap();
    state
        .lock()
        .expect("HTTP Exchange state mutex poisoned")
        .current
        .as_mut()
        .unwrap()
        .response = Some(Message::response(
        StatusCode::OK,
        &HeaderMap::new(),
        Bytes::from_static(b"policy-response"),
    ));
    {
        let mut state = state.lock().unwrap();
        state.current.as_mut().unwrap().response_actions = Some(vec![FaultAction::CustomStatus(
            StatusCode::from_u16(209).unwrap(),
        )]);
    }
    let mut reader = BufferedServerReader {
        state: Arc::clone(&state),
        response: Arc::new(Mutex::new(None)),
    };

    let read = reader.read().await.unwrap().unwrap();
    assert_eq!(read.body, "policy-response");

    let mut writer = BufferedAppWriter {
        state: Arc::clone(&state),
    };
    let write = tokio::spawn(async move {
        writer
            .write(HttpContext {
                header: read.header,
                body: "policy-response".into(),
                body_is_utf8: true,
                wire_body: b"policy-response".to_vec(),
            })
            .await
            .unwrap()
            .into_context()
    });

    assert_eq!(policy.responses.load(Ordering::SeqCst), 0);
    assert_eq!(
        policy
            .request_metadata
            .lock()
            .expect("request metadata mutex poisoned")
            .as_slice(),
        &[]
    );
    let output = output.await.unwrap().unwrap();
    let ResponseDisposition::Send { message, .. } = output.disposition else {
        panic!("expected response send disposition");
    };
    output.write_completed.send(Ok(())).unwrap();
    let written = write.await.unwrap();
    assert_eq!(message.body, Bytes::from_static(b"policy-response"));
    assert_eq!(message.http_status(), Some(209));
    assert_eq!(written.body, "policy-response");
    assert!(written.header.starts_with("HTTP/1.1 209"));
}

#[path = "tests/local_response.rs"]
mod local_response;

#[tokio::test]
async fn upstream_writer_commits_after_flush_while_reader_waits_for_response() {
    let policy = Arc::new(RecordingWirePolicy::default());
    let state = prepared_request_state(policy.clone());
    let response = Arc::new(Mutex::new(None));
    let started = Arc::new(tokio::sync::Notify::new());
    let release_response = Arc::new(tokio::sync::Notify::new());
    let connector = Arc::new(ControlledConnector {
        confirm_flush: true,
        started: Arc::clone(&started),
        release_response: Arc::clone(&release_response),
    });
    let mut writer =
        server_writer_with_response(Arc::clone(&state), policy, connector, Arc::clone(&response));
    let mut reader = BufferedServerReader { state, response };

    let writer_task = tokio::spawn(async move { writer.write(request_context()).await.unwrap() });
    started.notified().await;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(1), writer_task)
        .await
        .expect("flush confirms writer before response")
        .unwrap();
    assert!(matches!(outcome, WriteOutcome::Committed(_)));

    let mut read = Box::pin(reader.read());
    assert!(matches!(
        std::future::poll_fn(|context| std::task::Poll::Ready(read.as_mut().poll(context))).await,
        std::task::Poll::Pending
    ));
    release_response.notify_one();
    assert_eq!(read.await.unwrap().unwrap().body, "actor-response");
}

#[tokio::test]
async fn early_upstream_response_returns_read_ready_without_forging_flush_commit() {
    let policy = Arc::new(RecordingWirePolicy::default());
    let state = prepared_request_state(policy.clone());
    let response = Arc::new(Mutex::new(None));
    let connector = Arc::new(ControlledConnector {
        confirm_flush: false,
        started: Arc::new(tokio::sync::Notify::new()),
        release_response: Arc::new(tokio::sync::Notify::new()),
    });
    let mut writer =
        server_writer_with_response(Arc::clone(&state), policy, connector, Arc::clone(&response));
    let mut reader = BufferedServerReader { state, response };

    let outcome = writer.write(request_context()).await.unwrap();
    assert!(matches!(outcome, WriteOutcome::ReadReady(_)));
    assert_eq!(reader.read().await.unwrap().unwrap().body, "actor-response");
}

fn state(policy: Arc<RecordingWirePolicy>) -> Arc<Mutex<HttpExchangeState>> {
    Arc::new(Mutex::new(HttpExchangeState::new(
        connection_context(),
        policy,
        CancellationToken::new(),
    )))
}

fn prepared_request_state(policy: Arc<RecordingWirePolicy>) -> Arc<Mutex<HttpExchangeState>> {
    let state = state(policy);
    let (completed, _output) = tokio::sync::oneshot::channel();
    state
        .lock()
        .unwrap()
        .begin(command("server:80", request("prepared-request"), completed))
        .unwrap();
    state
        .lock()
        .unwrap()
        .current
        .as_mut()
        .unwrap()
        .request_actions = Some(Vec::new());
    state
}

fn request_context() -> HttpContext {
    HttpContext {
        header: "POST /sale HTTP/1.1\r\n\r\n".into(),
        body: "prepared-request".into(),
        body_is_utf8: true,
        wire_body: b"prepared-request".to_vec(),
    }
}

fn command(
    endpoint: &str,
    request: Message,
    completed: tokio::sync::oneshot::Sender<super::super::Result<HttpExchangeOutput>>,
) -> HttpExchangeCommand {
    HttpExchangeCommand {
        endpoint: endpoint.into(),
        request: HttpExchangeRequest {
            method: Method::POST,
            uri: "/sale".parse().unwrap(),
            message: request,
        },
        completed,
    }
}

fn request(body: &'static str) -> Message {
    Message {
        start_line: "POST /sale HTTP/1.1".into(),
        headers: Vec::new(),
        body: Bytes::from_static(body.as_bytes()),
        body_modified: false,
    }
}

fn connection_context() -> ConnectionContext {
    ConnectionContext {
        runtime_epoch: uuid::Uuid::from_u128(1),
        connection_id: uuid::Uuid::from_u128(2),
        channel: ChannelId::new("http-policy-order").unwrap(),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
        accepted_at: SystemTime::UNIX_EPOCH,
        tls_peer: None,
    }
}

fn server_writer(
    state: Arc<Mutex<HttpExchangeState>>,
    policy: Arc<RecordingWirePolicy>,
    connector: Arc<RecordingConnector>,
) -> BufferedServerWriter {
    server_writer_with_response(state, policy, connector, Arc::new(Mutex::new(None)))
}

fn server_writer_with_response(
    state: Arc<Mutex<HttpExchangeState>>,
    policy: Arc<RecordingWirePolicy>,
    connector: Arc<dyn UpstreamConnector>,
    response: Arc<Mutex<Option<oneshot::Receiver<super::super::Result<UpstreamExchange>>>>>,
) -> BufferedServerWriter {
    BufferedServerWriter {
        state,
        context: connection_context(),
        ports: policy,
        upstream: connector,
        clock: Arc::new(SystemClock),
        cancellation: CancellationToken::new(),
        informational: None,
        task_scope: ConnectionTaskScope::new(),
        response,
    }
}

#[test]
fn proxy_write_error_keeps_typed_external_package_failure() {
    let failure = intercept_proxy_exchange::ExternalPackageCallFailure {
        package: intercept_proxy_exchange::ProtocolPackageRef {
            id: intercept_proxy_exchange::ProtocolPackageId::new("phase10.http").unwrap(),
            version: intercept_proxy_exchange::ProtocolPackageVersion::new("1.0.0").unwrap(),
        },
        direction: intercept_proxy_exchange::ProtocolDirection::Upstream,
        stage: intercept_proxy_exchange::ExternalPackageCallStage::Encode,
        method: "hooks.upstream.encode".into(),
        request_id: Some("endpoint-encode-1".into()),
        remote_code: Some(-32_410),
        stable_code: Some("BODY_ENCODE_FAILED".into()),
        remote_message: Some("encode rejected".into()),
        remote_data_summary: Some("object(fields=1)".into()),
    };
    let error = proxy_write_error(
        crate::ProxyError::new(
            crate::ErrorCode::ExternalPackageCallFailed,
            "encode rejected",
        )
        .with_external_package_call(Some(Box::new(failure.clone()))),
    );

    assert_eq!(error.external_package_call.as_deref(), Some(&failure));
}
