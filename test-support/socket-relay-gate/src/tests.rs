use std::{sync::Arc, time::Duration};

use intercept_proxy_runtime::socket_relay::{
    SocketEndpoint, SocketRelayConfig, SocketRelaySecurity, SocketRelayService,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

use crate::{
    certificates::identity,
    harness::{Mode, REPLY, connect_retry, exercise_mode, payload, start_relay},
};

#[tokio::test]
async fn plain_transparent_preserves_exact_bytes_and_emits_evidence() {
    assert_mode(Mode::PlainTransparent).await;
}

#[tokio::test]
async fn tls_transparent_preserves_end_to_end_tls_and_emits_evidence() {
    assert_mode(Mode::TlsTransparent).await;
}

#[tokio::test]
async fn tcp_to_tls_preserves_exact_bytes_and_emits_evidence() {
    assert_mode(Mode::TcpToTls).await;
}

#[tokio::test]
async fn tls_to_tcp_preserves_exact_bytes_and_emits_evidence() {
    assert_mode(Mode::TlsToTcp).await;
}

#[tokio::test]
async fn tls_to_tls_preserves_exact_bytes_and_emits_evidence() {
    assert_mode(Mode::TlsToTls).await;
}

async fn assert_mode(mode: Mode) {
    let proxy = identity("gate proxy");
    let target = identity("gate target");
    let evidence = exercise_mode(mode, &proxy, &target).await;
    assert_eq!(evidence["mode"], mode.name());
    println!("{evidence}");
}

#[tokio::test]
async fn silent_connections_stop_and_release_the_port() {
    let upstream = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind silent target");
    let upstream_address = upstream.local_addr().expect("target address");
    let target_task = tokio::spawn(async move {
        let (_stream, _) = upstream.accept().await.expect("accept silent stream");
        std::future::pending::<()>().await;
    });
    let relay = start_relay(upstream_address, SocketRelaySecurity::Transparent).await;
    let silent = connect_retry(relay.address).await;
    let address = relay.address;
    relay.stop().await;
    drop(silent);
    target_task.abort();
    TcpListener::bind(address)
        .await
        .expect("rebind after silent stop");
}

#[tokio::test]
async fn silent_tls_handshake_stops_and_releases_the_port() {
    let upstream = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind silent TLS target");
    let upstream_address = upstream.local_addr().expect("target address");
    let target_task = tokio::spawn(async move {
        let (_stream, _) = upstream.accept().await.expect("accept silent TLS stream");
        std::future::pending::<()>().await;
    });
    let target = identity("silent TLS target");
    let relay = start_relay(
        upstream_address,
        SocketRelaySecurity::TcpToTls {
            upstream_tls: intercept_proxy_runtime::socket_relay::SocketUpstreamTlsConfig {
                server_trust_der: vec![target.ca],
                client_identity: None,
                verify_hostname: true,
                tls_server_name: None,
            },
        },
    )
    .await;
    let silent = connect_retry(relay.address).await;
    let address = relay.address;
    relay.stop().await;
    drop(silent);
    target_task.abort();
    TcpListener::bind(address)
        .await
        .expect("rebind after silent TLS stop");
}

#[tokio::test]
async fn dns_lookup_is_cancelled_on_stop_and_releases_the_port() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind DNS relay");
    let relay_address = listener.local_addr().expect("relay address");
    let service = Arc::new(
        SocketRelayService::build(SocketRelayConfig {
            bind_addr: relay_address,
            upstream: SocketEndpoint {
                host: "socket-relay-gate-does-not-exist.invalid".into(),
                port: 443,
            },
            security: SocketRelaySecurity::Transparent,
            maximum_connections: 2,
            read_chunk_bytes: 16 * 1024,
            connect_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
        })
        .expect("build DNS relay"),
    );
    let cancellation = CancellationToken::new();
    let task_service = Arc::clone(&service);
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        task_service
            .serve_prebound_listener(listener, task_cancellation)
            .await
    });
    let mut client = connect_retry(relay_address).await;
    client
        .write_all(b"begin DNS lookup")
        .await
        .expect("write DNS attempt");

    cancellation.cancel();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("DNS lookup cancellation stops relay")
        .expect("join DNS relay")
        .expect("stop DNS relay");
    drop(client);
    TcpListener::bind(relay_address)
        .await
        .expect("rebind after DNS stop");
}

#[tokio::test]
async fn upstream_half_close_keeps_client_to_server_direction_open_after_lazy_dial() {
    let upstream = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind server-first target");
    let upstream_address = upstream.local_addr().expect("target address");
    let expected = payload();
    let expected_len = expected.len();
    let trailing = b"after-upstream-half-close".to_vec();
    let target = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.expect("accept server-first");
        let mut initial = vec![0; expected_len];
        stream
            .read_exact(&mut initial)
            .await
            .expect("read initial App ingress");
        stream
            .write_all(REPLY)
            .await
            .expect("write server-first reply");
        stream.shutdown().await.expect("server-first half close");
        let mut received_after_half_close = Vec::new();
        stream
            .read_to_end(&mut received_after_half_close)
            .await
            .expect("read after half close");
        (initial, received_after_half_close)
    });
    let relay = start_relay(upstream_address, SocketRelaySecurity::Transparent).await;
    let mut client = connect_retry(relay.address).await;
    client
        .write_all(&expected)
        .await
        .expect("write initial App ingress");
    let mut reply = Vec::new();
    client
        .read_to_end(&mut reply)
        .await
        .expect("read server-first reply");
    assert_eq!(reply, REPLY);
    client
        .write_all(&trailing)
        .await
        .expect("write after server half close");
    client.shutdown().await.expect("client half close");
    assert_eq!(
        target.await.expect("join server-first target"),
        (expected, trailing)
    );
    relay.stop().await;
}

#[tokio::test]
async fn dns_and_connect_failure_recovers_on_the_next_connection() {
    let reservation = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve target");
    let upstream_address = reservation.local_addr().expect("target address");
    drop(reservation);
    let relay_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind relay");
    let relay_address = relay_listener.local_addr().expect("relay address");
    let service = Arc::new(
        SocketRelayService::build(SocketRelayConfig {
            bind_addr: relay_address,
            upstream: SocketEndpoint {
                host: "127.0.0.1".into(),
                port: upstream_address.port(),
            },
            security: SocketRelaySecurity::Transparent,
            maximum_connections: 4,
            read_chunk_bytes: 16 * 1024,
            connect_timeout: Duration::from_millis(150),
            read_timeout: Duration::from_secs(2),
            write_timeout: Duration::from_secs(2),
        })
        .expect("build relay"),
    );
    let cancellation = CancellationToken::new();
    let task_service = Arc::clone(&service);
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        task_service
            .serve_prebound_listener(relay_listener, task_cancellation)
            .await
    });

    let mut failed_client = connect_retry(relay_address).await;
    failed_client
        .write_all(b"first")
        .await
        .expect("write failed attempt");
    let mut closed = Vec::new();
    let close_result = tokio::time::timeout(
        Duration::from_secs(1),
        failed_client.read_to_end(&mut closed),
    )
    .await
    .expect("failed connection closes");
    if let Err(error) = close_result {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }

    let upstream = TcpListener::bind(upstream_address)
        .await
        .expect("start recovered target");
    let expected = payload();
    let expected_for_target = expected.clone();
    let target = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.expect("accept recovered");
        let mut received = Vec::new();
        stream
            .read_to_end(&mut received)
            .await
            .expect("read recovered");
        assert_eq!(received, expected_for_target);
        stream
            .write_all(b"recovered")
            .await
            .expect("reply recovered");
    });
    let mut recovered_client = TcpStream::connect(relay_address)
        .await
        .expect("connect recovered relay");
    recovered_client
        .write_all(&expected)
        .await
        .expect("write recovered payload");
    recovered_client
        .shutdown()
        .await
        .expect("half close recovered");
    let mut reply = Vec::new();
    recovered_client
        .read_to_end(&mut reply)
        .await
        .expect("read recovered reply");
    assert_eq!(reply, b"recovered");
    target.await.expect("join recovered target");
    cancellation.cancel();
    task.await.expect("join relay").expect("stop relay");
}
