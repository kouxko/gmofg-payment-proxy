//! Scripted/LocalResponder 集成测试的公共假件。
//!
//! 测试协议把第一个字节解释为 payload 长度，所以一个 Frame 的总长度为
//! `1 + payload_len`。处理器用方向标签替换长度字节，让断言能够证明输出确实经过了
//! 对应方向的 processor，而不是被透明转发。

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Decode, Direction, Display, Document, DocumentValue, Downstream, Encode, Error, Frame,
    FrameResult, JsonPointer, Rules, Socket, SocketContext, Upstream,
};
use tokio::{
    net::TcpStream,
    sync::{Barrier, Notify},
};

use super::super::{
    SocketConnectionEvent, SocketConnectionIdentity, SocketConnectionObserver,
    SocketDirectionCapabilities, SocketDownstreamSecurity, SocketEndpoint,
    SocketLocalResponderConfig, SocketObservationMetadata, SocketPayloadDirection,
    SocketPipelineLimits, SocketProcessingFailure, SocketProcessingFailureKind,
    SocketProtocolCapabilityFactory, SocketRelayConfig, SocketRelaySecurity,
};

pub(super) const TEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
pub(super) struct TestObserver {
    events: Mutex<Vec<SocketConnectionEvent>>,
    changed: Notify,
}

impl SocketConnectionObserver for TestObserver {
    fn record(&self, event: SocketConnectionEvent) {
        self.events.lock().unwrap().push(event);
        self.changed.notify_waiters();
    }
}

impl TestObserver {
    pub(super) fn events(&self) -> Vec<SocketConnectionEvent> {
        self.events.lock().unwrap().clone()
    }

    pub(super) async fn wait_until(&self, predicate: impl Fn(&SocketConnectionEvent) -> bool) {
        tokio::time::timeout(TEST_TIMEOUT, async {
            loop {
                let changed = self.changed.notified();
                if self.events.lock().unwrap().iter().any(&predicate) {
                    return;
                }
                changed.await;
            }
        })
        .await
        .expect("expected socket event was not observed before timeout");
    }
}

pub(super) async fn connect_retry(address: SocketAddr) -> TcpStream {
    TcpStream::connect(address)
        .await
        .unwrap_or_else(|error| panic!("connect to prebound socket listener {address}: {error}"))
}

pub(super) async fn bind_listener() -> (tokio::net::TcpListener, SocketAddr) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind socket test listener");
    let address = listener.local_addr().expect("socket test address");
    (listener, address)
}

pub(super) fn relay_config(bind_addr: SocketAddr, upstream: SocketAddr) -> SocketRelayConfig {
    SocketRelayConfig {
        bind_addr,
        upstream: SocketEndpoint {
            host: upstream.ip().to_string(),
            port: upstream.port(),
        },
        security: SocketRelaySecurity::Transparent,
        maximum_connections: 8,
        read_chunk_bytes: 16 * 1024,
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
    }
}

pub(super) fn local_config(bind_addr: SocketAddr) -> SocketLocalResponderConfig {
    SocketLocalResponderConfig {
        bind_addr,
        security: SocketDownstreamSecurity::Tcp,
        maximum_connections: 8,
        read_chunk_bytes: 16 * 1024,
        handshake_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
    }
}

pub(super) fn limits() -> SocketPipelineLimits {
    SocketPipelineLimits::new(1_024, 1_024, 7).unwrap()
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ProcessorOutcome {
    Transform,
    Fail,
    Panic,
}

struct TestFrame<D: Direction>(std::marker::PhantomData<fn() -> D>);

#[async_trait]
impl<D: Direction> Frame<D> for TestFrame<D> {
    async fn split(&mut self, buffered: &[u8]) -> Result<FrameResult, Error> {
        let total = usize::from(buffered[0]) + 1;
        Ok(if buffered.len() < total {
            FrameResult::NeedMore
        } else {
            FrameResult::Complete { consumed: total }
        })
    }
}

struct TestDecode<D: Direction> {
    barrier: Option<Arc<Barrier>>,
    marker: std::marker::PhantomData<fn() -> D>,
}

#[async_trait]
impl<D: Direction> Decode<Socket, D> for TestDecode<D> {
    async fn decode(&mut self, context: &SocketContext) -> Result<Document, Error> {
        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
        }
        wire_document(context.data.clone())
    }
}

struct TestDisplay;

#[async_trait]
impl Display for TestDisplay {
    async fn display(&mut self, _document: &Document) -> Result<String, Error> {
        Ok("test-frame".to_owned())
    }
}

struct TestRules<D: Direction> {
    tag: Option<u8>,
    outcome: ProcessorOutcome,
    marker: std::marker::PhantomData<fn() -> D>,
}

#[async_trait]
impl<D: Direction> Rules for TestRules<D> {
    async fn apply(&mut self, mut document: Document) -> Result<Document, Error> {
        match self.outcome {
            ProcessorOutcome::Transform => {
                if let Some(tag) = self.tag {
                    let mut data = blob(&document)?;
                    data[0] = tag;
                    document
                        .set(
                            &JsonPointer::property("data"),
                            DocumentValue::byte_array(data),
                        )
                        .map_err(|error| domain_error(&error))?;
                }
                Ok(document)
            }
            ProcessorOutcome::Fail => Err(Error::new(format!(
                "{:?}|{}: injected integration-test Rules failure",
                D::KIND,
                SocketProcessingFailureKind::ProcessingFailed.as_str()
            ))),
            ProcessorOutcome::Panic => Err(Error::new(format!(
                "{:?}|{}: injected integration-test capability panic",
                D::KIND,
                SocketProcessingFailureKind::ProcessorPanicked.as_str()
            ))),
        }
    }
}

struct TestEncode<D: Direction>(std::marker::PhantomData<fn() -> D>);

#[async_trait]
impl<D: Direction> Encode<Socket, D> for TestEncode<D> {
    async fn encode(
        &mut self,
        _original: &SocketContext,
        document: &Document,
    ) -> Result<SocketContext, Error> {
        Ok(SocketContext {
            data: blob(document)?,
        })
    }
}

fn capabilities<D: Direction>(
    tag: Option<u8>,
    outcome: ProcessorOutcome,
    barrier: Option<Arc<Barrier>>,
) -> SocketDirectionCapabilities<D> {
    SocketDirectionCapabilities::new(
        Box::new(TestFrame::<D>(std::marker::PhantomData)),
        Box::new(TestDecode::<D> {
            barrier,
            marker: std::marker::PhantomData,
        }),
        Box::new(TestDisplay),
        Box::new(TestRules::<D> {
            tag,
            outcome,
            marker: std::marker::PhantomData,
        }),
        Box::new(TestEncode::<D>(std::marker::PhantomData)),
    )
}

pub(super) struct ScriptedFactory {
    directions: Mutex<Vec<SocketPayloadDirection>>,
    barrier: Option<Arc<Barrier>>,
}

impl ScriptedFactory {
    pub(super) fn new(barrier: Option<Arc<Barrier>>) -> Self {
        Self {
            directions: Mutex::new(Vec::new()),
            barrier,
        }
    }

    pub(super) fn directions(&self) -> Vec<SocketPayloadDirection> {
        self.directions.lock().unwrap().clone()
    }
}

impl SocketProtocolCapabilityFactory for ScriptedFactory {
    fn observation_metadata(&self) -> SocketObservationMetadata {
        SocketObservationMetadata {
            workspace_id: "test-workspace".to_owned(),
            listener_id: "test-listener".to_owned(),
        }
    }

    fn create_upstream(
        &self,
        _connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Upstream>, SocketProcessingFailure> {
        self.directions
            .lock()
            .unwrap()
            .push(SocketPayloadDirection::AppToUpstream);
        Ok(capabilities(
            Some(b'U'),
            ProcessorOutcome::Transform,
            self.barrier.clone(),
        ))
    }

    fn create_downstream(
        &self,
        _connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Downstream>, SocketProcessingFailure> {
        self.directions
            .lock()
            .unwrap()
            .push(SocketPayloadDirection::UpstreamToApp);
        Ok(capabilities(
            Some(b'D'),
            ProcessorOutcome::Transform,
            self.barrier.clone(),
        ))
    }
}

pub(super) struct LocalFactory {
    created: AtomicUsize,
    outcome: ProcessorOutcome,
}

impl LocalFactory {
    pub(super) const fn new(outcome: ProcessorOutcome) -> Self {
        Self {
            created: AtomicUsize::new(0),
            outcome,
        }
    }

    pub(super) fn created(&self) -> usize {
        self.created.load(Ordering::SeqCst)
    }
}

impl SocketProtocolCapabilityFactory for LocalFactory {
    fn observation_metadata(&self) -> SocketObservationMetadata {
        SocketObservationMetadata {
            workspace_id: "test-workspace".to_owned(),
            listener_id: "test-listener".to_owned(),
        }
    }

    fn create_upstream(
        &self,
        _connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Upstream>, SocketProcessingFailure> {
        self.created.fetch_add(1, Ordering::SeqCst);
        Ok(capabilities(None, self.outcome, None))
    }

    fn create_downstream(
        &self,
        _connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Downstream>, SocketProcessingFailure> {
        self.created.fetch_add(1, Ordering::SeqCst);
        Ok(capabilities(None, self.outcome, None))
    }
}

fn wire_document(data: Vec<u8>) -> Result<Document, Error> {
    let mut document = Document::new(DocumentValue::Object(BTreeMap::default()));
    document
        .set(
            &JsonPointer::property("data"),
            DocumentValue::byte_array(data),
        )
        .map_err(|error| domain_error(&error))?;
    Ok(document)
}

fn blob(document: &Document) -> Result<Vec<u8>, Error> {
    match document
        .resolve(&JsonPointer::property("data"))
        .map_err(|error| domain_error(&error))?
    {
        DocumentValue::Array(values) => values
            .iter()
            .map(|value| match value {
                DocumentValue::Number(number)
                    if number.get().fract() == 0.0 && (0.0..=255.0).contains(&number.get()) =>
                {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    Ok(number.get() as u8)
                }
                _ => Err(Error::new("test document data must contain byte numbers")),
            })
            .collect(),
        _ => Err(Error::new("test document data must be an array")),
    }
}

fn domain_error(error: &intercept_proxy_exchange::DomainError) -> Error {
    Error::new(format!("{}: {error}", error.code.as_str()))
}
