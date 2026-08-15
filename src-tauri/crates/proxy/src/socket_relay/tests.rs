use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

use super::{
    SocketConnectionEvent, SocketConnectionObserver, SocketEndpoint, SocketRejectionReason,
    SocketRelayConfig, SocketRelayDirection, SocketRelayRunContext, SocketRelaySecurity,
    SocketRelayService, SocketRelayStage,
};

#[derive(Debug, Default)]
struct RecordingObserver(Mutex<Vec<SocketConnectionEvent>>);

impl SocketConnectionObserver for RecordingObserver {
    fn record(&self, event: SocketConnectionEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn reserve_address() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    address
}

fn transparent_config(bind_addr: SocketAddr, upstream: SocketAddr) -> SocketRelayConfig {
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

async fn connect_retry(address: SocketAddr) -> TcpStream {
    for _ in 0..40 {
        if let Ok(stream) = TcpStream::connect(address).await {
            return stream;
        }
        tokio::task::yield_now().await;
    }
    panic!("socket listener did not start at {address}");
}

async fn wait_for_event(
    observer: &RecordingObserver,
    predicate: impl Fn(&SocketConnectionEvent) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if observer.0.lock().unwrap().iter().any(&predicate) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("expected socket event was not observed before timeout");
}

#[tokio::test]
async fn transparent_relay_preserves_binary_and_asymmetric_half_close() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let payload = Arc::new(
        (0..40_000_u32)
            .map(|index| index.wrapping_mul(131).to_le_bytes()[0])
            .collect::<Vec<_>>(),
    );
    let expected = Arc::clone(&payload);
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut received = Vec::new();
        stream.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, *expected);
        stream.write_all(b"reply\0\xff").await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let bind_addr = reserve_address();
    let service = Arc::new(
        SocketRelayService::build(transparent_config(bind_addr, upstream_address)).unwrap(),
    );
    let cancellation = CancellationToken::new();
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { service.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    client.write_all(&payload).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, b"reply\0\xff");

    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.await.unwrap();
    TcpListener::bind(bind_addr).await.unwrap();
}

#[tokio::test]
async fn transparent_mode_relays_an_end_to_end_tls_client_hello_opaquely() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let expected = b"\x16\x03\x01\0\x05hello";
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut bytes = [0_u8; 10];
        stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, expected);
        stream.write_all(b"opaque-tls-reply").await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let bind_addr = reserve_address();
    let service = Arc::new(
        SocketRelayService::build(transparent_config(bind_addr, upstream_address)).unwrap(),
    );
    let cancellation = CancellationToken::new();
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { service.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    client.write_all(expected).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, b"opaque-tls-reply");
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.await.unwrap();
}

#[test]
fn endpoint_and_capacity_validation_reject_url_shaped_hosts_and_invalid_limits() {
    for host in ["", " example.com", "https://example.com", "a/b", "a?b=1"] {
        assert!(
            SocketEndpoint {
                host: host.into(),
                port: 443
            }
            .validate()
            .is_err()
        );
    }
    let mut config = transparent_config(
        "127.0.0.1:1".parse().unwrap(),
        "127.0.0.1:2".parse().unwrap(),
    );
    config.maximum_connections = 0;
    assert!(config.validate().is_err());
    config.maximum_connections = 5_001;
    assert!(config.validate().is_err());
}

mod direct;
mod local_responder;
mod scripted_relay;
mod support;
mod tls;

#[tokio::test]
async fn cancellation_emits_one_terminal_event_and_resets_active_metrics() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (_stream, _) = upstream.accept().await.unwrap();
        std::future::pending::<()>().await;
    });
    let bind_addr = reserve_address();
    let observer = Arc::new(RecordingObserver::default());
    let service = Arc::new(
        SocketRelayService::build_with_observer(
            transparent_config(bind_addr, upstream_address),
            observer.clone(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let run = SocketRelayRunContext {
        listener_id: "socket-test-listener".into(),
        workspace_runtime_epoch: uuid::Uuid::new_v4(),
        listener_run_epoch: uuid::Uuid::new_v4(),
    };
    let expected_run = run.clone();
    let server = tokio::spawn(async move { running.serve_with_context(run, server_cancel).await });
    let _client = connect_retry(bind_addr).await;
    for _ in 0..100 {
        if observer
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, SocketConnectionEvent::Opened { .. }))
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.abort();

    {
        let events = observer.0.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, SocketConnectionEvent::Admitted { .. }))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, SocketConnectionEvent::Opened { .. }))
                .count(),
            1
        );
        let closed = events
            .iter()
            .filter_map(|event| match event {
                SocketConnectionEvent::Closed { failure, .. } => {
                    Some(failure.as_ref().map(|failure| failure.code))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(closed, vec![Some("SOCKET_RELAY_CANCELLED")]);
        assert!(events.iter().all(|event| match event {
            SocketConnectionEvent::Admitted { run, .. }
            | SocketConnectionEvent::Opened { run, .. }
            | SocketConnectionEvent::RequestParsed { run, .. }
            | SocketConnectionEvent::Closed { run, .. }
            | SocketConnectionEvent::Rejected { run, .. } => run == &expected_run,
        }));
    }
    assert_eq!(service.metrics().await.active_connections, 0);
}

#[tokio::test]
async fn cidr_and_capacity_rejections_are_typed_and_counted() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (_stream, _) = upstream.accept().await.unwrap();
        std::future::pending::<()>().await;
    });

    let bind_addr = reserve_address();
    let observer = Arc::new(RecordingObserver::default());
    let mut config = transparent_config(bind_addr, upstream_address);
    config.maximum_connections = 1;
    let service =
        Arc::new(SocketRelayService::build_with_observer(config, observer.clone()).unwrap());
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });
    let _first = connect_retry(bind_addr).await;
    wait_for_event(&observer, |event| {
        matches!(event, SocketConnectionEvent::Opened { .. })
    })
    .await;
    let _second = connect_retry(bind_addr).await;
    wait_for_event(&observer, |event| {
        matches!(
            event,
            SocketConnectionEvent::Rejected {
                reason: SocketRejectionReason::Capacity,
                code: "SOCKET_CAPACITY_EXHAUSTED",
                ..
            }
        )
    })
    .await;
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.abort();
    assert_eq!(service.metrics().await.rejected_connections, 1);
}

#[tokio::test]
async fn pre_open_dns_and_connect_failures_have_typed_stages() {
    for (host, port, expected_stage, expected_codes) in [
        (
            "does-not-exist.invalid".to_owned(),
            443,
            SocketRelayStage::Dns,
            &["SOCKET_DNS_FAILED", "SOCKET_DNS_TIMEOUT"][..],
        ),
        (
            "127.0.0.1".to_owned(),
            reserve_address().port(),
            SocketRelayStage::Connect,
            &["SOCKET_CONNECT_FAILED", "SOCKET_CONNECT_TIMEOUT"][..],
        ),
    ] {
        let bind_addr = reserve_address();
        let observer = Arc::new(RecordingObserver::default());
        let mut config = transparent_config(bind_addr, "127.0.0.1:1".parse().unwrap());
        config.upstream = SocketEndpoint { host, port };
        let service =
            Arc::new(SocketRelayService::build_with_observer(config, observer.clone()).unwrap());
        let cancellation = CancellationToken::new();
        let running = Arc::clone(&service);
        let server_cancel = cancellation.clone();
        let server = tokio::spawn(async move { running.serve(server_cancel).await });
        let _client = connect_retry(bind_addr).await;
        wait_for_event(&observer, |event| match event {
            SocketConnectionEvent::Closed {
                opened: false,
                failure: Some(failure),
                ..
            } => failure.stage == expected_stage && expected_codes.contains(&failure.code),
            _ => false,
        })
        .await;
        cancellation.cancel();
        server.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn read_timeout_is_directional_and_terminal_is_unique() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (_stream, _) = upstream.accept().await.unwrap();
        std::future::pending::<()>().await;
    });
    let bind_addr = reserve_address();
    let observer = Arc::new(RecordingObserver::default());
    let mut config = transparent_config(bind_addr, upstream_address);
    config.read_timeout = Duration::from_millis(10);
    let service =
        Arc::new(SocketRelayService::build_with_observer(config, observer.clone()).unwrap());
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });
    let _client = connect_retry(bind_addr).await;
    wait_for_event(&observer, |event| {
        matches!(event, SocketConnectionEvent::Closed { .. })
    })
    .await;
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.abort();

    let events = observer.0.lock().unwrap();
    let closed = events
        .iter()
        .filter_map(|event| match event {
            SocketConnectionEvent::Closed { failure, .. } => failure.as_ref(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].stage, SocketRelayStage::RelayRead);
    assert_eq!(closed[0].code, "SOCKET_READ_TIMEOUT");
    assert!(matches!(
        closed[0].direction,
        Some(SocketRelayDirection::ClientToServer | SocketRelayDirection::ServerToClient)
    ));
}
