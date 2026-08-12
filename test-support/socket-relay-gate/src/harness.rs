use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_runtime::socket_relay::{
    BoundedSocketConnectionObserver, SocketConnectionEvent, SocketDownstreamTlsConfig,
    SocketEndpoint, SocketRelayConfig, SocketRelaySecurity, SocketRelayService,
    SocketUpstreamTlsConfig,
};
use ring::digest::{SHA256, digest};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::certificates::{TestIdentity, accept_tls, connect_tls, socket_identity};

pub(crate) const REPLY: &[u8] = b"socket-relay-reply\0\xff";

#[derive(Clone, Copy, Debug)]
pub(crate) enum Mode {
    PlainTransparent,
    TlsTransparent,
    TcpToTls,
    TlsToTcp,
    TlsToTls,
}

impl Mode {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::PlainTransparent => "plain-transparent",
            Self::TlsTransparent => "tls-transparent",
            Self::TcpToTls => "tcp-to-tls",
            Self::TlsToTcp => "tls-to-tcp",
            Self::TlsToTls => "tls-to-tls",
        }
    }

    fn downstream_tls(self) -> bool {
        matches!(self, Self::TlsTransparent | Self::TlsToTcp | Self::TlsToTls)
    }

    fn upstream_tls(self) -> bool {
        matches!(self, Self::TlsTransparent | Self::TcpToTls | Self::TlsToTls)
    }
}

pub(crate) struct RunningRelay {
    pub(crate) address: SocketAddr,
    service: Arc<SocketRelayService>,
    cancellation: CancellationToken,
    task: JoinHandle<intercept_proxy_runtime::Result<()>>,
    observer: Arc<BoundedSocketConnectionObserver>,
}

impl RunningRelay {
    pub(crate) async fn stop(self) {
        self.cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(2), self.task)
            .await
            .expect("relay stops within deadline")
            .expect("join relay")
            .expect("stop relay");
        assert_eq!(self.service.metrics().await.active_connections, 0);
    }

    async fn terminal_evidence(&self) -> (u64, u64) {
        for _ in 0..100 {
            if let Some(bytes) = successful_close_bytes(&self.observer) {
                return bytes;
            }
            tokio::task::yield_now().await;
        }
        panic!("Socket observer did not record a successful terminal event");
    }
}

fn successful_close_bytes(observer: &BoundedSocketConnectionObserver) -> Option<(u64, u64)> {
    observer.snapshot().iter().find_map(|event| match event {
        SocketConnectionEvent::Closed {
            opened: true,
            bytes,
            failure: None,
            ..
        } => Some((bytes.client_to_server, bytes.server_to_client)),
        _ => None,
    })
}

pub(crate) fn payload() -> Vec<u8> {
    (0..70_000_u32)
        .map(|index| index.wrapping_mul(131).to_le_bytes()[0])
        .collect()
}

pub(crate) async fn exercise_mode(
    mode: Mode,
    proxy_identity: &TestIdentity,
    target_identity: &TestIdentity,
) -> Value {
    let expected = Arc::new(payload());
    let upstream = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind upstream");
    let upstream_address = upstream.local_addr().expect("upstream address");
    let target_identity_for_task = target_identity.clone();
    let expected_for_task = Arc::clone(&expected);
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.expect("accept upstream");
        if mode.upstream_tls() {
            echo_after_eof(
                accept_tls(stream, &target_identity_for_task).await,
                expected_for_task,
            )
            .await
        } else {
            echo_after_eof(stream, expected_for_task).await
        }
    });
    let security = security(mode, proxy_identity, target_identity);
    let relay = start_relay(upstream_address, security).await;
    let stream = connect_retry(relay.address).await;
    let peer_fingerprint = if mode.downstream_tls() {
        let stream = connect_tls(
            stream,
            if matches!(mode, Mode::TlsTransparent) {
                target_identity
            } else {
                proxy_identity
            },
        )
        .await;
        let certificate = stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|certificates| certificates.first())
            .expect("peer certificate");
        let fingerprint = sha256(certificate.as_ref());
        (roundtrip(stream, &expected).await, Some(fingerprint))
    } else {
        (roundtrip(stream, &expected).await, None)
    };
    let (client_reply, peer_fingerprint) = peer_fingerprint;
    let upstream_received = upstream_task.await.expect("join upstream");
    let observed = relay.terminal_evidence().await;
    let metrics = relay.service.metrics().await;
    assert_eq!(
        observed,
        (
            metrics.client_to_server_bytes,
            metrics.server_to_client_bytes,
        )
    );
    if !matches!(mode, Mode::TlsTransparent) {
        assert_eq!(
            observed,
            (upstream_received.len() as u64, client_reply.len() as u64)
        );
    }
    let proxy_address = relay.address;
    relay.stop().await;
    let rebound = TcpListener::bind(proxy_address)
        .await
        .expect("immediate rebind");
    drop(rebound);
    json!({
        "mode": mode.name(),
        "proxy_endpoint": proxy_address,
        "upstream_endpoint": upstream_address,
        "client_to_server": {
            "bytes": upstream_received.len(),
            "sha256": sha256(&upstream_received),
        },
        "server_to_client": {
            "bytes": client_reply.len(),
            "sha256": sha256(&client_reply),
        },
        "relay_observer": {
            "client_to_server_bytes": observed.0,
            "server_to_client_bytes": observed.1,
        },
        "metrics": {
            "client_to_server_bytes": metrics.client_to_server_bytes,
            "server_to_client_bytes": metrics.server_to_client_bytes,
        },
        "peer_fingerprint": peer_fingerprint,
        "terminal": "success",
        "port_rebind": true,
    })
}

pub(crate) async fn start_relay(
    upstream: SocketAddr,
    security: SocketRelaySecurity,
) -> RunningRelay {
    let reservation = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve relay address");
    let address = reservation.local_addr().expect("relay address");
    drop(reservation);
    let observer = Arc::new(BoundedSocketConnectionObserver::new(32));
    let service = Arc::new(
        SocketRelayService::build_with_observer(
            SocketRelayConfig {
                bind_addr: address,
                allowed_client_cidrs: Vec::new(),
                upstream: SocketEndpoint {
                    host: "127.0.0.1".into(),
                    port: upstream.port(),
                },
                security,
                maximum_connections: 16,
                connect_timeout: Duration::from_millis(500),
                read_timeout: Duration::from_secs(2),
                write_timeout: Duration::from_secs(2),
            },
            observer.clone(),
        )
        .expect("build relay"),
    );
    let cancellation = CancellationToken::new();
    let task_service = Arc::clone(&service);
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move { task_service.serve(task_cancellation).await });
    RunningRelay {
        address,
        service,
        cancellation,
        task,
        observer,
    }
}

fn security(mode: Mode, proxy: &TestIdentity, target: &TestIdentity) -> SocketRelaySecurity {
    let downstream_tls = SocketDownstreamTlsConfig {
        server_identity: socket_identity(proxy),
        client_trust_der: Vec::new(),
        client_authentication_required: false,
    };
    let upstream_tls = SocketUpstreamTlsConfig {
        server_trust_der: vec![target.ca.clone()],
        client_identity: None,
        verify_hostname: true,
    };
    match mode {
        Mode::PlainTransparent | Mode::TlsTransparent => SocketRelaySecurity::Transparent,
        Mode::TcpToTls => SocketRelaySecurity::TcpToTls { upstream_tls },
        Mode::TlsToTcp => SocketRelaySecurity::TlsToTcp { downstream_tls },
        Mode::TlsToTls => SocketRelaySecurity::TlsToTls {
            downstream_tls,
            upstream_tls,
        },
    }
}

pub(crate) async fn connect_retry(address: SocketAddr) -> TcpStream {
    for _ in 0..100 {
        if let Ok(stream) = TcpStream::connect(address).await {
            return stream;
        }
        tokio::task::yield_now().await;
    }
    panic!("relay did not bind {address}");
}

async fn echo_after_eof<S>(mut stream: S, expected: Arc<Vec<u8>>) -> Vec<u8>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut received = Vec::new();
    stream
        .read_to_end(&mut received)
        .await
        .expect("read payload");
    assert_eq!(received, *expected);
    stream.write_all(REPLY).await.expect("write reply");
    stream.shutdown().await.expect("shutdown reply");
    received
}

async fn roundtrip<S>(mut stream: S, expected: &[u8]) -> Vec<u8>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream.write_all(expected).await.expect("write payload");
    stream.shutdown().await.expect("half close client");
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).await.expect("read reply");
    assert_eq!(reply, REPLY);
    reply
}

fn sha256(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
