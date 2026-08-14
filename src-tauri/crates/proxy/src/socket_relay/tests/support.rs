//! Scripted/LocalResponder 集成测试的公共假件。
//!
//! 测试协议把第一个字节解释为 payload 长度，所以一个 Frame 的总长度为
//! `1 + payload_len`。处理器用方向标签替换长度字节，让断言能够证明输出确实经过了
//! 对应方向的 processor，而不是被透明转发。

use std::{
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use tokio::{net::TcpStream, sync::Barrier};

use super::super::{
    FrameBoundary, LocalResponderProcessorFactory, ScriptedRelayProcessorFactory,
    SocketConnectionEvent, SocketConnectionIdentity, SocketConnectionObserver,
    SocketDownstreamSecurity, SocketEndpoint, SocketFrameProcessor, SocketFramePumpLimits,
    SocketLocalResponderConfig, SocketPayloadDirection, SocketProcessingFailure,
    SocketProcessingFailureKind, SocketRelayConfig, SocketRelaySecurity,
};

pub(super) const TEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
pub(super) struct TestObserver(Mutex<Vec<SocketConnectionEvent>>);

impl SocketConnectionObserver for TestObserver {
    fn record(&self, event: SocketConnectionEvent) {
        self.0.lock().unwrap().push(event);
    }
}

impl TestObserver {
    pub(super) fn events(&self) -> Vec<SocketConnectionEvent> {
        self.0.lock().unwrap().clone()
    }

    pub(super) async fn wait_until(&self, predicate: impl Fn(&SocketConnectionEvent) -> bool) {
        tokio::time::timeout(TEST_TIMEOUT, async {
            loop {
                if self.0.lock().unwrap().iter().any(&predicate) {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("expected socket event was not observed before timeout");
    }
}

pub(super) fn reserve_address() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    address
}

pub(super) async fn connect_retry(address: SocketAddr) -> TcpStream {
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            if let Ok(stream) = TcpStream::connect(address).await {
                return stream;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("socket listener did not start before timeout")
}

pub(super) fn relay_config(bind_addr: SocketAddr, upstream: SocketAddr) -> SocketRelayConfig {
    SocketRelayConfig {
        bind_addr,
        allowed_client_cidrs: Vec::new(),
        upstream: SocketEndpoint {
            host: upstream.ip().to_string(),
            port: upstream.port(),
        },
        security: SocketRelaySecurity::Transparent,
        maximum_connections: 8,
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
    }
}

pub(super) fn local_config(bind_addr: SocketAddr) -> SocketLocalResponderConfig {
    SocketLocalResponderConfig {
        bind_addr,
        allowed_client_cidrs: Vec::new(),
        security: SocketDownstreamSecurity::Tcp,
        maximum_connections: 8,
        handshake_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
    }
}

pub(super) fn limits() -> SocketFramePumpLimits {
    SocketFramePumpLimits::new(1_024, 1_024, 7, Duration::from_secs(1)).unwrap()
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ProcessorOutcome {
    Transform,
    Fail,
    Panic,
}

pub(super) struct LengthPrefixedProcessor {
    tag: u8,
    outcome: ProcessorOutcome,
    barrier: Option<Arc<Barrier>>,
}

impl LengthPrefixedProcessor {
    fn new(tag: u8, outcome: ProcessorOutcome, barrier: Option<Arc<Barrier>>) -> Self {
        Self {
            tag,
            outcome,
            barrier,
        }
    }
}

#[async_trait]
impl SocketFrameProcessor for LengthPrefixedProcessor {
    async fn inspect(&mut self, buffered: Bytes) -> Result<FrameBoundary, SocketProcessingFailure> {
        let total = usize::from(buffered[0]) + 1;
        if buffered.len() < total {
            Ok(FrameBoundary::NeedMore { total })
        } else {
            Ok(FrameBoundary::Complete { bytes: total })
        }
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        if let Some(barrier) = &self.barrier {
            barrier.wait().await;
        }
        match self.outcome {
            ProcessorOutcome::Transform => {
                let mut output = Vec::with_capacity(origin.len());
                output.push(self.tag);
                output.extend_from_slice(&origin[1..]);
                Ok(Bytes::from(output))
            }
            ProcessorOutcome::Fail => Err(SocketProcessingFailure::new(
                SocketProcessingFailureKind::ProcessingFailed,
                "injected integration-test processor failure",
            )),
            ProcessorOutcome::Panic => panic!("injected integration-test processor panic"),
        }
    }
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

impl ScriptedRelayProcessorFactory for ScriptedFactory {
    fn create_direction(
        &self,
        _connection: SocketConnectionIdentity,
        direction: SocketPayloadDirection,
    ) -> Box<dyn SocketFrameProcessor> {
        self.directions.lock().unwrap().push(direction);
        let tag = match direction {
            SocketPayloadDirection::AppToUpstream => b'U',
            SocketPayloadDirection::UpstreamToApp => b'D',
            SocketPayloadDirection::LocalExchange => unreachable!(),
        };
        Box::new(LengthPrefixedProcessor::new(
            tag,
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

impl LocalResponderProcessorFactory for LocalFactory {
    fn create_exchange(
        &self,
        _connection: SocketConnectionIdentity,
    ) -> Box<dyn SocketFrameProcessor> {
        self.created.fetch_add(1, Ordering::SeqCst);
        Box::new(LengthPrefixedProcessor::new(b'R', self.outcome, None))
    }
}
