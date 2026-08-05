use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use intercept_proxy_runtime::RuntimeServiceFactory;
use intercept_proxy_runtime::message::{Message, MessageLimits};
use intercept_proxy_runtime::supervisor::{ChannelConfig, ChannelId, ProxyConfig, ProxyState};
use intercept_proxy_runtime::transport::{
    AcceptedConnection, BoxIo, ConnectionAcceptor, ConnectionContext, ConnectionService,
    ForwardRequest, HandshakePolicy, HyperUpstreamConnector, NoopPipelinePorts, PipelinePorts,
    UpstreamExchange,
};
use intercept_proxy_runtime::{
    ConnectionAdmission, ErrorCode, FaultAction, ProxyError, ProxySupervisor, Result, SystemClock,
    TokioListenerBinder, UpstreamConnector,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Default)]
struct RecordingPorts {
    bodies: Mutex<Vec<Bytes>>,
    messages: Mutex<Vec<Message>>,
    connection_ids: Mutex<Vec<Uuid>>,
}

impl fmt::Debug for RecordingPorts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("RecordingPorts").finish()
    }
}

impl HandshakePolicy for RecordingPorts {}

#[async_trait]
impl PipelinePorts for RecordingPorts {
    async fn connection_opened(&self, context: &ConnectionContext) {
        self.connection_ids
            .lock()
            .unwrap()
            .push(context.connection_id);
    }

    async fn request(
        &self,
        _context: &ConnectionContext,
        message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        self.bodies.lock().unwrap().push(message.passthrough_body());
        self.messages.lock().unwrap().push(message.clone());
        Ok(Vec::new())
    }
}

#[derive(Debug)]
struct TestPlaintextAcceptor;

#[async_trait]
impl ConnectionAcceptor for TestPlaintextAcceptor {
    async fn accept(&self, io: BoxIo, _context: &ConnectionContext) -> Result<AcceptedConnection> {
        Ok(AcceptedConnection { io, tls_peer: None })
    }
}

#[derive(Default)]
struct LifecyclePorts {
    events: Mutex<Vec<&'static str>>,
    opened: Notify,
    closed: Notify,
}

impl fmt::Debug for LifecyclePorts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("LifecyclePorts").finish()
    }
}

impl HandshakePolicy for LifecyclePorts {}

#[async_trait]
impl PipelinePorts for LifecyclePorts {
    async fn runtime_stopping(&self, _epoch: Uuid) {
        self.events.lock().unwrap().push("runtime_stopping");
    }

    async fn connection_opened(&self, _context: &ConnectionContext) {
        self.events.lock().unwrap().push("connection_opened");
        self.opened.notify_one();
    }

    async fn connection_closed(&self, _context: &ConnectionContext, _result: &Result<()>) {
        self.events.lock().unwrap().push("connection_closed");
        self.closed.notify_one();
    }
}

#[derive(Debug)]
struct EchoConnector;

#[async_trait]
impl UpstreamConnector for EchoConnector {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        request: ForwardRequest,
        _actions: &[FaultAction],
        _informational: Option<&intercept_proxy_runtime::transport::InformationalResponseSink>,
        _cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
        Ok(Message::response(
            StatusCode::OK,
            &HeaderMap::new(),
            request.message.passthrough_body(),
        )
        .into())
    }
}

#[derive(Debug)]
struct RawResponseConnector;

#[async_trait]
impl UpstreamConnector for RawResponseConnector {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        _request: ForwardRequest,
        _actions: &[FaultAction],
        _informational: Option<&intercept_proxy_runtime::transport::InformationalResponseSink>,
        _cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
        Message::from_raw_http1_head(
            b"HTTP/1.1 299 Vendor Specific Result\r\n\
X-Trace: first\x80\r\n\
x-Other: middle\xff\r\n\
x-TRACE: second\r\n\
x-Other: last\r\n\
Content-Length: 2\r\n\r\n",
            Bytes::from_static(b"ok"),
        )
        .map(Into::into)
    }
}

#[derive(Debug)]
struct ResponseFaultPorts(Vec<FaultAction>);

impl HandshakePolicy for ResponseFaultPorts {}

#[async_trait]
impl PipelinePorts for ResponseFaultPorts {
    async fn response(
        &self,
        _context: &ConnectionContext,
        _message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        Ok(self.0.clone())
    }
}

#[derive(Default)]
struct ClosedResultPorts {
    request_actions: Vec<FaultAction>,
    closed_code: Mutex<Option<String>>,
    closed: Notify,
}

impl fmt::Debug for ClosedResultPorts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ClosedResultPorts").finish()
    }
}

impl HandshakePolicy for ClosedResultPorts {}

#[async_trait]
impl PipelinePorts for ClosedResultPorts {
    async fn request(
        &self,
        _context: &ConnectionContext,
        _message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        Ok(self.request_actions.clone())
    }

    async fn connection_closed(&self, _context: &ConnectionContext, result: &Result<()>) {
        *self.closed_code.lock().unwrap() =
            result.as_ref().err().map(|error| error.code.to_owned());
        self.closed.notify_one();
    }
}

#[derive(Debug)]
struct FailingConnector(ErrorCode);

#[async_trait]
impl UpstreamConnector for FailingConnector {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        _request: ForwardRequest,
        _actions: &[FaultAction],
        _informational: Option<&intercept_proxy_runtime::transport::InformationalResponseSink>,
        _cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
        Err(ProxyError::new(self.0, "injected connector failure"))
    }
}

#[derive(Debug)]
struct RecordingFactory {
    calls: AtomicUsize,
    snapshots: Mutex<Vec<ProxyConfig>>,
}

#[async_trait]
impl RuntimeServiceFactory for RecordingFactory {
    async fn build(&self, config: &ProxyConfig) -> Result<BTreeMap<ChannelId, ConnectionService>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.snapshots.lock().unwrap().push(config.clone());
        Ok(config
            .channels
            .iter()
            .filter(|channel| channel.enabled)
            .map(|channel| {
                (
                    channel.channel.clone(),
                    service(Arc::new(NoopPipelinePorts)),
                )
            })
            .collect())
    }
}

fn channel_id(value: &str) -> ChannelId {
    ChannelId::new(value).expect("valid test channel ID")
}

fn service(ports: Arc<dyn PipelinePorts>) -> ConnectionService {
    service_with_connector(ports, Arc::new(EchoConnector))
}

fn service_with_connector(
    ports: Arc<dyn PipelinePorts>,
    upstream: Arc<dyn UpstreamConnector>,
) -> ConnectionService {
    ConnectionService {
        acceptor: Arc::new(TestPlaintextAcceptor),
        upstream,
        ports,
        clock: Arc::new(SystemClock),
        admission: ConnectionAdmission::new(500).unwrap(),
        limits: MessageLimits::default(),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    }
}

fn config() -> ProxyConfig {
    ProxyConfig {
        channels: vec![
            ChannelConfig {
                channel: channel_id("alpha"),
                enabled: true,
                listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                upstream_url: "http://alpha.test/".into(),
            },
            ChannelConfig {
                channel: channel_id("beta"),
                enabled: true,
                listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                upstream_url: "http://beta.test/".into(),
            },
        ],
        limits: MessageLimits::default(),
        max_connections: 500,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(2),
        rewrite_host: true,
        leaf_sans: vec!["localhost".into()],
    }
}

async fn exchange(address: SocketAddr, body: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).await.unwrap();
    let head = format!(
        "POST /settle HTTP/1.1\r\nHost: app\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    let mut received = Vec::new();
    let read_result =
        tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut received))
            .await
            .expect("server closes the HTTP/1.1 connection");
    if let Err(error) = read_result {
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
        ));
    }
    received
}

async fn exchange_raw(address: SocketAddr, request: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut received))
        .await
        .expect("server closes the HTTP/1.1 connection")
        .expect("read raw response");
    received
}

async fn read_http_head(stream: &mut TcpStream) -> Vec<u8> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.expect("HTTP head byte");
        head.push(byte[0]);
    }
    head
}
