//! Scripted Relay 的真实 TCP 集成测试。
//!
//! 这些测试位于 service 边界外侧，同时连接 App 侧与固定上游，覆盖 factory 创建、
//! 双方向独立执行、方向内 FIFO，以及 observer 只发布一次终态。

use std::sync::Arc;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

use super::{
    super::{SocketConnectionEvent, SocketPayloadDirection, SocketRelayService},
    support::{
        ScriptedFactory, TEST_TIMEOUT, TestObserver, bind_listener, connect_retry, limits,
        relay_config,
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

    let (listener, bind_addr) = bind_listener().await;
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
    let server = tokio::spawn(async move {
        running
            .serve_listener(listener, uuid::Uuid::new_v4(), server_cancel)
            .await
    });

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
async fn scripted_exchange_processes_multiple_interactions_sequentially() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        for (request_payload, response_payload) in [(b'c', b's'), (b'd', b't')] {
            let mut request = [0_u8; 2];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(request, [b'U', request_payload]);
            stream.write_all(&[1, response_payload]).await.unwrap();
        }
        stream.shutdown().await.unwrap();
    });

    let (listener, bind_addr) = bind_listener().await;
    let factory = Arc::new(ScriptedFactory::new(None));
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
    let server = tokio::spawn(async move {
        running
            .serve_listener(listener, uuid::Uuid::new_v4(), server_cancel)
            .await
    });

    tokio::time::timeout(TEST_TIMEOUT, async {
        let mut client = connect_retry(bind_addr).await;
        client.write_all(&[1, b'c']).await.unwrap();
        let mut first = [0_u8; 2];
        client.read_exact(&mut first).await.unwrap();
        assert_eq!(&first, b"Ds");
        client.write_all(&[1, b'd']).await.unwrap();
        client.shutdown().await.unwrap();
        let mut second = Vec::new();
        client.read_to_end(&mut second).await.unwrap();
        assert_eq!(second, b"Dt");
        upstream_task.await.unwrap();
    })
    .await
    .expect("sequential request-response interactions must not deadlock");

    cancellation.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn scripted_exchange_rejects_pipelined_app_frames_and_emits_one_terminal_event() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();

    let (listener, bind_addr) = bind_listener().await;
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
    let server = tokio::spawn(async move {
        running
            .serve_listener(listener, uuid::Uuid::new_v4(), server_cancel)
            .await
    });

    let mut client = connect_retry(bind_addr).await;
    client.write_all(&[1, b'a', 1, b'b']).await.unwrap();
    client.shutdown().await.unwrap();
    let _ = client.read_to_end(&mut Vec::new()).await;
    observer
        .wait_until(|event| matches!(event, SocketConnectionEvent::Closed { .. }))
        .await;
    cancellation.cancel();
    server.await.unwrap().unwrap();

    // 同一次 transport read 已包含一个完整 Frame 之外的数据，Reader Pipeline 必须在
    // 建立固定 Server connection 之前失败，不能先转发第一帧再丢弃尾部。
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), upstream.accept())
            .await
            .is_err()
    );

    assert_eq!(
        observer
            .events()
            .iter()
            .filter(|event| matches!(event, SocketConnectionEvent::Closed { .. }))
            .count(),
        1
    );
    assert!(observer.events().iter().any(|event| matches!(
        event,
        SocketConnectionEvent::Closed {
            failure: Some(_),
            ..
        }
    )));
}
