//! Scripted Relay 的真实 TCP 集成测试。
//!
//! 这些测试位于 service 边界外侧，同时连接 App 侧与固定上游，覆盖 factory 创建、
//! 双方向独立执行、方向内 FIFO，以及 observer 只发布一次终态。

use std::sync::Arc;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Barrier,
};
use tokio_util::sync::CancellationToken;

use super::{
    super::{SocketConnectionEvent, SocketPayloadDirection, SocketRelayService},
    support::{
        ScriptedFactory, TEST_TIMEOUT, TestObserver, connect_retry, limits, relay_config,
        reserve_address,
    },
};

#[tokio::test]
async fn scripted_relay_transforms_both_directions_and_creates_each_processor_once() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = [0_u8; 4];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"Uabc");
        stream.write_all(&[3, b'x', b'y', b'z']).await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let bind_addr = reserve_address();
    let factory = Arc::new(ScriptedFactory::new(None));
    let observer = Arc::new(TestObserver::default());
    let service = Arc::new(
        SocketRelayService::build_scripted_with_observer(
            relay_config(bind_addr, upstream_address),
            factory.clone(),
            limits(),
            observer.clone(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    client.write_all(&[3, b'a', b'b', b'c']).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, b"Dxyz");

    observer
        .wait_until(|event| matches!(event, SocketConnectionEvent::Closed { .. }))
        .await;
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.await.unwrap();
    let mut directions = factory.directions();
    directions.sort_by_key(|direction| match direction {
        SocketPayloadDirection::AppToUpstream => 0,
        SocketPayloadDirection::UpstreamToApp => 1,
        SocketPayloadDirection::LocalExchange => 2,
    });
    assert_eq!(
        directions,
        [
            SocketPayloadDirection::AppToUpstream,
            SocketPayloadDirection::UpstreamToApp,
        ]
    );
}

#[tokio::test]
async fn scripted_relay_processes_opposite_directions_concurrently() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        stream.write_all(&[1, b's']).await.unwrap();
        let mut request = [0_u8; 2];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"Uc");
        stream.shutdown().await.unwrap();
    });

    let bind_addr = reserve_address();
    let factory = Arc::new(ScriptedFactory::new(Some(Arc::new(Barrier::new(2)))));
    let service = Arc::new(
        SocketRelayService::build_scripted(
            relay_config(bind_addr, upstream_address),
            factory,
            limits(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    tokio::time::timeout(TEST_TIMEOUT, async {
        let mut client = connect_retry(bind_addr).await;
        client.write_all(&[1, b'c']).await.unwrap();
        client.shutdown().await.unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert_eq!(response, b"Ds");
        upstream_task.await.unwrap();
    })
    .await
    .expect("both direction processors must reach the barrier concurrently");

    cancellation.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn scripted_relay_preserves_frame_fifo_and_emits_one_terminal_event() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = Vec::new();
        stream.read_to_end(&mut request).await.unwrap();
        assert_eq!(request, b"UaUbUccc");
        stream.shutdown().await.unwrap();
    });

    let bind_addr = reserve_address();
    let observer = Arc::new(TestObserver::default());
    let service = Arc::new(
        SocketRelayService::build_scripted_with_observer(
            relay_config(bind_addr, upstream_address),
            Arc::new(ScriptedFactory::new(None)),
            limits(),
            observer.clone(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    client
        .write_all(&[1, b'a', 1, b'b', 3, b'c', b'c', b'c'])
        .await
        .unwrap();
    client.shutdown().await.unwrap();
    client.read_to_end(&mut Vec::new()).await.unwrap();
    observer
        .wait_until(|event| matches!(event, SocketConnectionEvent::Closed { .. }))
        .await;
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.await.unwrap();

    assert_eq!(
        observer
            .events()
            .iter()
            .filter(|event| matches!(event, SocketConnectionEvent::Closed { .. }))
            .count(),
        1
    );
}
