//! Raw TCP/HTTP integration coverage for TEST-PROXY and TEST-CONCURRENCY.

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use gmofg_proxy_runtime::RuntimeServiceFactory;
use gmofg_proxy_runtime::message::{Message, MessageLimits};
use gmofg_proxy_runtime::supervisor::{Channel, ChannelConfig, ProxyConfig, ProxyState};
use gmofg_proxy_runtime::transport::{
    AcceptedConnection, BoxIo, ConnectionAcceptor, ConnectionContext, ConnectionService,
    ForwardRequest, HandshakePolicy, NoopPipelinePorts, PipelinePorts,
};
use gmofg_proxy_runtime::{
    ErrorCode, FaultAction, ProxyError, ProxySupervisor, Result, SystemClock, TokioListenerBinder,
    UpstreamConnector,
};
use http::{HeaderMap, StatusCode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Default)]
struct RecordingPorts {
    bodies: Mutex<Vec<Bytes>>,
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
    }
}

#[derive(Debug)]
struct EchoConnector;

#[async_trait]
impl UpstreamConnector for EchoConnector {
    async fn send(
        &self,
        request: ForwardRequest,
        _actions: &[FaultAction],
        _cancellation: &CancellationToken,
    ) -> Result<Message> {
        Ok(Message::response(
            StatusCode::OK,
            &HeaderMap::new(),
            request.message.passthrough_body(),
        ))
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
        _request: ForwardRequest,
        _actions: &[FaultAction],
        _cancellation: &CancellationToken,
    ) -> Result<Message> {
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
    async fn build(&self, config: &ProxyConfig) -> Result<BTreeMap<Channel, ConnectionService>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.snapshots.lock().unwrap().push(config.clone());
        Ok(config
            .channels
            .iter()
            .filter(|channel| channel.enabled)
            .map(|channel| (channel.channel, service(Arc::new(NoopPipelinePorts))))
            .collect())
    }
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
        limits: MessageLimits::default(),
        read_timeout: Duration::from_secs(2),
    }
}

fn config() -> ProxyConfig {
    ProxyConfig {
        channels: vec![
            ChannelConfig {
                channel: Channel::Transaction,
                enabled: true,
                listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                upstream_url: "http://transaction.test/".into(),
            },
            ChannelConfig {
                channel: Channel::Dll,
                enabled: true,
                listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                upstream_url: "http://dll.test/".into(),
            },
        ],
        limits: MessageLimits::default(),
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

#[tokio::test]
async fn request_handler_preserves_stable_fault_error_codes_on_connection_close() {
    let cases = [
        (
            vec![FaultAction::DisconnectBeforeUpstream],
            None,
            ErrorCode::ClientDisconnected.as_str(),
        ),
        (
            Vec::new(),
            Some(ErrorCode::UpstreamConnectTimeout),
            ErrorCode::UpstreamConnectTimeout.as_str(),
        ),
    ];

    for (request_actions, connector_error, expected_code) in cases {
        let ports = Arc::new(ClosedResultPorts {
            request_actions,
            ..ClosedResultPorts::default()
        });
        let upstream: Arc<dyn UpstreamConnector> = connector_error.map_or_else(
            || Arc::new(EchoConnector) as Arc<dyn UpstreamConnector>,
            |code| Arc::new(FailingConnector(code)) as Arc<dyn UpstreamConnector>,
        );
        let supervisor = ProxySupervisor::new(
            Arc::new(TokioListenerBinder),
            service_with_connector(ports.clone(), upstream),
        );
        let started = supervisor.start(config()).await.unwrap();
        let _response = exchange(started.listeners[&Channel::Dll], b"request").await;
        tokio::time::timeout(Duration::from_secs(1), ports.closed.notified())
            .await
            .expect("connection closed callback");
        assert_eq!(
            ports.closed_code.lock().unwrap().as_deref(),
            Some(expected_code)
        );
        supervisor.stop().await.unwrap();
    }
}

#[tokio::test]
async fn two_ports_use_http11_close_and_preserve_body_bytes() {
    let ports = Arc::new(RecordingPorts::default());
    let supervisor = ProxySupervisor::new(Arc::new(TokioListenerBinder), service(ports.clone()));
    let started = supervisor.start(config()).await.unwrap();
    assert_eq!(started.state, ProxyState::Running);
    assert_eq!(started.listeners.len(), 2);

    let raw_body = [0x81, 0x00, 0xff];
    for address in started.listeners.values() {
        let response = exchange(*address, &raw_body).await;
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(
            response
                .windows(b"connection: close".len())
                .any(|window| window.eq_ignore_ascii_case(b"connection: close"))
        );
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        assert_eq!(&response[split + 4..], &raw_body);
    }

    {
        let captured = ports.bodies.lock().unwrap();
        assert_eq!(
            captured.as_slice(),
            &[
                Bytes::copy_from_slice(&raw_body),
                Bytes::copy_from_slice(&raw_body)
            ]
        );
    }
    {
        let connection_ids = ports.connection_ids.lock().unwrap();
        assert_eq!(connection_ids.len(), 2);
        assert_ne!(connection_ids[0], connection_ids[1]);
        assert!(connection_ids.iter().all(|id| !id.is_nil()));
    }
    assert_eq!(supervisor.stop().await.unwrap().state, ProxyState::Stopped);
}

#[tokio::test]
async fn runtime_stopping_is_delivered_before_active_connections_join() {
    let ports = Arc::new(LifecyclePorts::default());
    let supervisor = ProxySupervisor::new(Arc::new(TokioListenerBinder), service(ports.clone()));
    let started = supervisor.start(config()).await.unwrap();
    let _client = TcpStream::connect(started.listeners[&Channel::Transaction])
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), ports.opened.notified())
        .await
        .expect("connection opened");

    supervisor.stop().await.unwrap();

    let events = ports.events.lock().unwrap();
    let stopping = events
        .iter()
        .position(|event| *event == "runtime_stopping")
        .unwrap();
    let closed = events
        .iter()
        .position(|event| *event == "connection_closed")
        .unwrap();
    assert!(stopping < closed, "events: {events:?}");
}

#[tokio::test]
async fn stop_cancels_twenty_active_clients() {
    let supervisor = Arc::new(ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service(Arc::new(NoopPipelinePorts)),
    ));
    let started = supervisor.start(config()).await.unwrap();
    let address = started.listeners[&Channel::Transaction];
    let mut clients = Vec::new();
    for _ in 0..20 {
        clients.push(TcpStream::connect(address).await.unwrap());
    }
    assert_eq!(supervisor.stop().await.unwrap().state, ProxyState::Stopped);
    for mut client in clients {
        let mut byte = [0u8; 1];
        let result = tokio::time::timeout(Duration::from_secs(1), client.read(&mut byte))
            .await
            .expect("client connection is cancelled");
        match result {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ) => {}
            other => panic!("unexpected client result after stop: {other:?}"),
        }
    }
}

#[tokio::test]
async fn second_listener_bind_failure_rolls_back_first_listener() {
    let first_reservation = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let first = first_reservation.local_addr().unwrap();
    drop(first_reservation);
    let occupied = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let second = occupied.local_addr().unwrap();
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service(Arc::new(NoopPipelinePorts)),
    );
    let config = ProxyConfig {
        channels: vec![
            ChannelConfig {
                channel: Channel::Transaction,
                enabled: true,
                listen_addr: first,
                upstream_url: "http://transaction.test/".into(),
            },
            ChannelConfig {
                channel: Channel::Dll,
                enabled: true,
                listen_addr: second,
                upstream_url: "http://dll.test/".into(),
            },
        ],
        limits: MessageLimits::default(),
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(2),
        rewrite_host: true,
        leaf_sans: vec!["localhost".into()],
    };

    let error = supervisor.start(config).await.unwrap_err();
    assert_eq!(error.code, "PORT_IN_USE");
    assert_eq!(supervisor.snapshot().await.state, ProxyState::Faulted);
    let rebound = tokio::net::TcpListener::bind(first)
        .await
        .expect("first transactional bind was rolled back");
    drop(rebound);
    drop(occupied);
}

#[tokio::test]
async fn truncation_sends_only_prefix_then_closes() {
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service(Arc::new(ResponseFaultPorts(vec![
            FaultAction::TruncateResponse(3),
        ]))),
    );
    let started = supervisor.start(config()).await.unwrap();
    let response = exchange(started.listeners[&Channel::Transaction], b"abcdef").await;
    assert!(!response.is_empty(), "proxy returned no response bytes");
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    assert_eq!(&response[split + 4..], b"abc");
    assert!(
        response[..split]
            .windows(b"content-length: 6".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-length: 6"))
    );
    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn short_declared_length_still_writes_full_wire_body() {
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service(Arc::new(ResponseFaultPorts(vec![
            FaultAction::ContentLengthOffset(-3),
        ]))),
    );
    let started = supervisor.start(config()).await.unwrap();
    let response = exchange(started.listeners[&Channel::Transaction], b"abcdef").await;
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    assert_eq!(&response[split + 4..], b"abcdef");
    assert!(
        response[..split]
            .windows(b"content-length: 3".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-length: 3"))
    );
    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn runtime_factory_receives_one_complete_snapshot_per_epoch() {
    let factory = Arc::new(RecordingFactory {
        calls: AtomicUsize::new(0),
        snapshots: Mutex::new(Vec::new()),
    });
    let supervisor = ProxySupervisor::with_factory(Arc::new(TokioListenerBinder), factory.clone());
    let expected = config();
    let first_epoch = supervisor
        .start(expected.clone())
        .await
        .unwrap()
        .runtime_epoch
        .unwrap();
    assert_eq!(factory.calls.load(Ordering::Relaxed), 1);
    {
        let snapshots = factory.snapshots.lock().unwrap();
        assert_eq!(
            snapshots[0].channels[0].upstream_url,
            expected.channels[0].upstream_url
        );
        assert_eq!(snapshots[0].connect_timeout, expected.connect_timeout);
        assert_eq!(snapshots[0].write_timeout, expected.write_timeout);
        assert_eq!(snapshots[0].read_timeout, expected.read_timeout);
        assert_eq!(snapshots[0].rewrite_host, expected.rewrite_host);
        assert_eq!(snapshots[0].leaf_sans, expected.leaf_sans);
    }
    supervisor.stop().await.unwrap();
    let second_epoch = supervisor
        .start(expected)
        .await
        .unwrap()
        .runtime_epoch
        .unwrap();
    assert_ne!(first_epoch, second_epoch);
    assert_eq!(factory.calls.load(Ordering::Relaxed), 2);
    supervisor.stop().await.unwrap();
}
