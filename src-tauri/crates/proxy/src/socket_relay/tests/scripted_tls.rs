//! Scripted Relay 与 TLS transport 组合的真实 loopback 回归。
//!
//! Direct Relay 的 TLS 桥接不能证明协议流水线也经过同一 transport。这里使用测试协议
//! （长度字节 + payload），明确断言 App→Server 与 Server→App 都先完成 TLS，再分别经过
//! Upstream/Downstream processor。

use std::sync::Arc;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

use super::{
    super::{
        SocketConnectionEvent, SocketDownstreamTlsConfig, SocketRelaySecurity, SocketRelayService,
        SocketRelayStage, SocketUpstreamTlsConfig,
    },
    support::{ScriptedFactory, TestObserver, bind_listener, connect_retry, limits, relay_config},
    tls::{Identity, identity, mtls_accept, socket_identity, tls_accept, tls_connect_result},
};

#[tokio::test]
async fn scripted_tcp_to_tls_transforms_both_directions() {
    let upstream_identity = identity("scripted tcp to tls upstream", false);
    let (upstream, upstream_address) = tls_upstream(upstream_identity.clone()).await;
    let (listener, bind_addr) = bind_listener().await;
    let mut config = relay_config(bind_addr, upstream_address);
    config.security = SocketRelaySecurity::TcpToTls {
        upstream_tls: upstream_tls(&upstream_identity, None),
    };

    let response = scripted_roundtrip(listener, config, None).await;

    assert_eq!(response, b"Dr");
    upstream.await.unwrap();
}

#[tokio::test]
async fn scripted_tls_to_tcp_transforms_both_directions() {
    let proxy_identity = identity("scripted tls to tcp proxy", false);
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        assert_scripted_request_and_reply(&mut stream).await;
    });
    let (listener, bind_addr) = bind_listener().await;
    let mut config = relay_config(bind_addr, upstream_address);
    config.security = SocketRelaySecurity::TlsToTcp {
        downstream_tls: downstream_tls(&proxy_identity, None, false),
    };

    let response = scripted_roundtrip(listener, config, Some((&proxy_identity, None))).await;

    assert_eq!(response, b"Dr");
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn scripted_tls_to_tls_accepts_trusted_identities_on_both_sides() {
    let proxy_identity = identity("scripted mutual proxy", false);
    let app_identity = identity("scripted trusted app", true);
    let upstream_identity = identity("scripted mutual upstream", false);
    let proxy_client_identity = identity("scripted trusted proxy", true);
    let (upstream, upstream_address) =
        mtls_upstream(upstream_identity.clone(), proxy_client_identity.ca.clone()).await;
    let (listener, bind_addr) = bind_listener().await;
    let mut config = relay_config(bind_addr, upstream_address);
    config.security = SocketRelaySecurity::TlsToTls {
        downstream_tls: downstream_tls(&proxy_identity, Some(&app_identity), true),
        upstream_tls: upstream_tls(&upstream_identity, Some(&proxy_client_identity)),
    };

    let response = scripted_roundtrip(
        listener,
        config,
        Some((&proxy_identity, Some(&app_identity))),
    )
    .await;

    assert_eq!(response, b"Dr");
    upstream.await.unwrap();
}

#[tokio::test]
async fn scripted_tls_to_tls_rejects_an_app_without_required_identity_before_upstream_dial() {
    let proxy_identity = identity("scripted required app identity proxy", false);
    let app_identity = identity("scripted required app identity", true);
    let upstream_identity = identity("scripted untouched upstream", false);
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let (listener, bind_addr) = bind_listener().await;
    let mut config = relay_config(bind_addr, upstream_address);
    config.security = SocketRelaySecurity::TlsToTls {
        downstream_tls: downstream_tls(&proxy_identity, Some(&app_identity), true),
        upstream_tls: upstream_tls(&upstream_identity, None),
    };
    let service = Arc::new(
        SocketRelayService::build_scripted(config, Arc::new(ScriptedFactory::new(None)), limits())
            .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move {
        running
            .serve_listener(listener, uuid::Uuid::new_v4(), server_cancel)
            .await
    });

    let tcp = connect_retry(bind_addr).await;
    let rejected = tls_connect_result(tcp, &proxy_identity.ca, None).await;

    assert!(rejected.is_err());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), upstream.accept())
            .await
            .is_err(),
        "a rejected App TLS handshake must not dial the upstream server"
    );
    cancellation.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn scripted_tls_to_tls_fails_closed_when_upstream_requires_missing_client_identity() {
    let proxy_identity = identity("scripted upstream auth proxy", false);
    let upstream_identity = identity("scripted upstream requires identity", false);
    let required_identity = identity("scripted required proxy identity", true);
    let (upstream, upstream_address) =
        rejecting_mtls_upstream(upstream_identity.clone(), required_identity.ca.clone()).await;
    let (listener, bind_addr) = bind_listener().await;
    let observer = Arc::new(TestObserver::default());
    let mut config = relay_config(bind_addr, upstream_address);
    config.security = SocketRelaySecurity::TlsToTls {
        downstream_tls: downstream_tls(&proxy_identity, None, false),
        upstream_tls: upstream_tls(&upstream_identity, None),
    };
    let service = Arc::new(
        SocketRelayService::build_scripted_with_observer(
            config,
            Arc::new(ScriptedFactory::new(None)),
            limits(),
            observer.clone(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move {
        running
            .serve_listener(listener, uuid::Uuid::new_v4(), server_cancel)
            .await
    });
    let tcp = connect_retry(bind_addr).await;
    let mut app = tls_connect_result(tcp, &proxy_identity.ca, None)
        .await
        .unwrap();

    app.write_all(&[1, b'q']).await.unwrap();
    app.shutdown().await.unwrap();
    let mut response = Vec::new();
    app.read_to_end(&mut response).await.unwrap();
    observer
        .wait_until(|event| matches!(event, SocketConnectionEvent::Closed { .. }))
        .await;

    assert!(response.is_empty());
    assert!(observer.events().iter().any(|event| matches!(
        event,
        SocketConnectionEvent::Closed {
            failure: Some(failure),
            ..
        } if failure.stage == SocketRelayStage::UpstreamTls
    )));
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream.await.unwrap();
}

async fn scripted_roundtrip(
    listener: TcpListener,
    config: super::super::SocketRelayConfig,
    downstream_tls: Option<(&Identity, Option<&Identity>)>,
) -> Vec<u8> {
    let bind_addr = config.bind_addr;
    let service = Arc::new(
        SocketRelayService::build_scripted(config, Arc::new(ScriptedFactory::new(None)), limits())
            .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move {
        running
            .serve_listener(listener, uuid::Uuid::new_v4(), server_cancel)
            .await
    });
    let tcp = connect_retry(bind_addr).await;
    let mut response = Vec::new();
    if let Some((server_identity, client_identity)) = downstream_tls {
        let mut client = tls_connect_result(tcp, &server_identity.ca, client_identity)
            .await
            .unwrap();
        exchange(&mut client, &mut response).await;
    } else {
        let mut client = tcp;
        exchange(&mut client, &mut response).await;
    }
    cancellation.cancel();
    server.await.unwrap().unwrap();
    response
}

async fn exchange<S>(stream: &mut S, response: &mut Vec<u8>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    stream.write_all(&[1, b'q']).await.unwrap();
    stream.shutdown().await.unwrap();
    stream.read_to_end(response).await.unwrap();
}

async fn tls_upstream(identity: Identity) -> (tokio::task::JoinHandle<()>, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = tls_accept(stream, &identity).await;
        assert_scripted_request_and_reply(&mut stream).await;
    });
    (task, address)
}

async fn mtls_upstream(
    identity: Identity,
    trusted_ca: Vec<u8>,
) -> (tokio::task::JoinHandle<()>, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = mtls_accept(stream, &identity, &trusted_ca).await.unwrap();
        assert_scripted_request_and_reply(&mut stream).await;
    });
    (task, address)
}

async fn rejecting_mtls_upstream(
    identity: Identity,
    trusted_ca: Vec<u8>,
) -> (tokio::task::JoinHandle<()>, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        assert!(mtls_accept(stream, &identity, &trusted_ca).await.is_err());
    });
    (task, address)
}

async fn assert_scripted_request_and_reply<S>(stream: &mut S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut request = [0_u8; 2];
    stream.read_exact(&mut request).await.unwrap();
    assert_eq!(&request, b"Uq");
    stream.write_all(&[1, b'r']).await.unwrap();
    stream.shutdown().await.unwrap();
}

fn downstream_tls(
    server: &Identity,
    trusted_client: Option<&Identity>,
    required: bool,
) -> SocketDownstreamTlsConfig {
    SocketDownstreamTlsConfig {
        server_identity: socket_identity(server),
        client_trust_der: trusted_client
            .map(|identity| vec![identity.ca.clone()])
            .unwrap_or_default(),
        client_authentication_required: required,
    }
}

fn upstream_tls(server: &Identity, client: Option<&Identity>) -> SocketUpstreamTlsConfig {
    SocketUpstreamTlsConfig {
        server_trust_der: vec![server.ca.clone()],
        client_identity: client.map(socket_identity),
        verify_hostname: true,
        tls_server_name: None,
    }
}
