use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use intercept_proxy_exchange::{
    DirectionKind, Document, Downstream, Error, ErrorStage, ExternalPackageCallFailure,
    ExternalPackageCallStage, HttpContext, ProtocolDirection, ProtocolPackageId,
    ProtocolPackageRef, ProtocolPackageVersion, Upstream,
};
use tokio_util::sync::CancellationToken;

use super::{exchange_error, finalized_upstream_uri};
use crate::http::{
    ChannelId, Clock, ConnectionContext, FaultAction, Finalize, ForwardRequest, HandshakePolicy,
    HttpConnectionIdentity, HttpDirectionCapabilities, HttpExchangeRequest, HttpExchangeRuntime,
    HttpMessagePrepare, HttpObservationMetadata, HttpProtocolCapabilityFactory,
    InformationalResponseSink, PipelinePorts, PlainHttpCapabilityFactory, SystemClock,
    UpstreamConnector, UpstreamExchange, UpstreamWriteReceipt,
};
use crate::listener::ConnectionTaskScope;
use crate::{ErrorCode, HttpRequestMetadata, Message};

#[test]
fn exchange_error_preserves_adapter_error_code() {
    let error = exchange_error(Error::coded(
        "UPSTREAM_READ_TIMEOUT",
        "server reply timed out",
    ));

    assert_eq!(error.code, ErrorCode::UpstreamReadTimeout.as_str());
    assert!(error.message.contains("server reply timed out"));
}

#[test]
fn exchange_error_maps_core_failure_to_internal() {
    let error = exchange_error(Error::new("Server disconnected before replying"));

    assert_eq!(error.code, ErrorCode::Internal.as_str());
    assert!(
        error
            .message
            .contains("Server disconnected before replying")
    );
}

#[test]
fn exchange_error_preserves_typed_external_package_failure() {
    let failure = ExternalPackageCallFailure {
        package: ProtocolPackageRef {
            id: ProtocolPackageId::new("phase10.http").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        },
        direction: ProtocolDirection::Upstream,
        stage: ExternalPackageCallStage::Display,
        method: "document.upstream.display".into(),
        request_id: Some("proxy-4".into()),
        remote_code: Some(-32412),
        stable_code: Some("DISPLAY_FAILED".into()),
        remote_message: Some("display rejected".into()),
        remote_data_summary: Some("object(fields=1)".into()),
    };
    let error = exchange_error(
        Error::coded("EXTERNAL_PACKAGE_CALL_FAILED", "display rejected")
            .with_external_package_call(failure.clone()),
    );
    assert_eq!(error.code, "EXTERNAL_PACKAGE_CALL_FAILED");
    assert_ne!(error.code, ErrorCode::Internal.as_str());
    assert_eq!(error.external_package_call.as_deref(), Some(&failure));
}

#[test]
fn finalized_upstream_uri_changes_only_path_and_query() {
    let outgoing = "/old?amount=1".parse().unwrap();
    let request = HttpRequestMetadata {
        method: "POST".into(),
        request_target: "/old?amount=1".into(),
        url: "https://payments.example/old?amount=1".into(),
    };
    let message = Message {
        start_line: "POST https://payments.example/new?amount=2 HTTP/1.1".into(),
        headers: Vec::new(),
        body: bytes::Bytes::new(),
        body_modified: false,
    };

    assert_eq!(
        finalized_upstream_uri(&outgoing, &request, &message).unwrap(),
        "/new?amount=2"
    );
}

#[test]
fn finalized_upstream_uri_rejects_method_and_origin_changes() {
    let outgoing = "/old".parse().unwrap();
    let request = HttpRequestMetadata {
        method: "POST".into(),
        request_target: "/old".into(),
        url: "https://payments.example/old".into(),
    };
    for start_line in [
        "PUT https://payments.example/new HTTP/1.1",
        "POST http://payments.example/new HTTP/1.1",
        "POST https://other.example/new HTTP/1.1",
        "POST /new HTTP/1.1",
    ] {
        let message = Message {
            start_line: start_line.into(),
            headers: Vec::new(),
            body: bytes::Bytes::new(),
            body_modified: false,
        };
        assert!(finalized_upstream_uri(&outgoing, &request, &message).is_err());
    }
}

#[derive(Debug, Default)]
struct LegacyMockPolicy;

impl HandshakePolicy for LegacyMockPolicy {}

#[async_trait]
impl PipelinePorts for LegacyMockPolicy {
    async fn apply_request_policy(
        &self,
        _context: &ConnectionContext,
        _request: &HttpRequestMetadata,
        _message: &mut Message,
    ) -> crate::Result<Vec<FaultAction>> {
        Ok(vec![FaultAction::MockResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Bytes::from_static(b"mocked"),
        }])
    }
}

#[derive(Debug)]
struct LegacyMockCapabilities {
    prepare_calls: Arc<AtomicUsize>,
    finalize_calls: Arc<AtomicUsize>,
}

impl HttpProtocolCapabilityFactory for LegacyMockCapabilities {
    fn observation_metadata(&self) -> HttpObservationMetadata {
        HttpObservationMetadata {
            workspace_id: "test".into(),
            listener_id: "test".into(),
        }
    }

    fn create_upstream(
        &self,
        connection: HttpConnectionIdentity,
    ) -> Result<HttpDirectionCapabilities<Upstream>, Error> {
        PlainHttpCapabilityFactory::new("test", "test").create_upstream(connection)
    }

    fn create_downstream(
        &self,
        connection: HttpConnectionIdentity,
    ) -> Result<HttpDirectionCapabilities<Downstream>, Error> {
        let mut capabilities =
            PlainHttpCapabilityFactory::new("test", "test").create_downstream(connection)?;
        capabilities.prepare = Box::new(CountingDownstreamPrepare {
            calls: Arc::clone(&self.prepare_calls),
        });
        capabilities.finalize = Box::new(FailingLegacyMockFinalize {
            finalize_calls: Arc::clone(&self.finalize_calls),
        });
        Ok(capabilities)
    }
}

struct CountingDownstreamPrepare {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl HttpMessagePrepare<Downstream> for CountingDownstreamPrepare {
    async fn prepare(
        &mut self,
        _connection: &ConnectionContext,
        _request: &HttpRequestMetadata,
        _message: &mut Message,
        _original: &HttpContext,
        _document: &Document,
        _ports: &dyn PipelinePorts,
    ) -> Result<Vec<FaultAction>, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

struct FailingLegacyMockFinalize {
    finalize_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Finalize for FailingLegacyMockFinalize {
    async fn finalize(
        &mut self,
        _request: &HttpRequestMetadata,
        _message: &mut Message,
    ) -> Result<(), Error> {
        self.finalize_calls.fetch_add(1, Ordering::SeqCst);
        Err(Error::coded(
            "MOCK_FINALIZE_FAILED",
            "legacy Mock Finalize failed",
        ))
    }
}

#[derive(Debug)]
struct NoWriteConnector(Arc<AtomicUsize>);

#[async_trait]
impl UpstreamConnector for NoWriteConnector {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        _request: ForwardRequest,
        _actions: &[FaultAction],
        _write_receipt: UpstreamWriteReceipt,
        _informational: Option<&InformationalResponseSink>,
        _cancellation: &CancellationToken,
    ) -> crate::Result<UpstreamExchange> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(crate::ProxyError::new(
            ErrorCode::Internal,
            "legacy Mock must not reach the upstream connector",
        ))
    }
}

#[tokio::test]
async fn legacy_mock_finalize_failure_is_finalize_stage_and_writes_nothing() {
    let prepare_calls = Arc::new(AtomicUsize::new(0));
    let finalize_calls = Arc::new(AtomicUsize::new(0));
    let upstream_writes = Arc::new(AtomicUsize::new(0));
    let context = ConnectionContext {
        runtime_epoch: uuid::Uuid::from_u128(1),
        connection_id: uuid::Uuid::from_u128(2),
        channel: ChannelId::new("legacy-mock-finalize").unwrap(),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
        accepted_at: std::time::SystemTime::UNIX_EPOCH,
        tls_peer: None,
    };
    let task_scope = ConnectionTaskScope::new();
    let exchange = HttpExchangeRuntime {
        context,
        ports: Arc::new(LegacyMockPolicy),
        upstream: Arc::new(NoWriteConnector(Arc::clone(&upstream_writes))),
        clock: Arc::new(SystemClock) as Arc<dyn Clock>,
        cancellation: CancellationToken::new(),
        informational: None,
        capabilities: Arc::new(LegacyMockCapabilities {
            prepare_calls: Arc::clone(&prepare_calls),
            finalize_calls: Arc::clone(&finalize_calls),
        }),
        endpoint: "example.test:80".into(),
    }
    .start(&task_scope)
    .unwrap();

    let result = exchange
        .exchange(
            "example.test:80".into(),
            HttpExchangeRequest {
                method: http::Method::POST,
                uri: "/payment".parse().unwrap(),
                message: Message {
                    start_line: "POST /payment HTTP/1.1".into(),
                    headers: Vec::new(),
                    body: Bytes::from_static(b"request"),
                    body_modified: false,
                },
            },
        )
        .await;
    let Err(error) = result else {
        panic!("legacy Mock Finalize failure must stop before App write");
    };

    assert_eq!(error.stage, Some(ErrorStage::Finalize));
    assert_eq!(error.direction, Some(DirectionKind::Downstream));
    assert_eq!(prepare_calls.load(Ordering::SeqCst), 0);
    assert_eq!(finalize_calls.load(Ordering::SeqCst), 1);
    assert_eq!(upstream_writes.load(Ordering::SeqCst), 0);
}
