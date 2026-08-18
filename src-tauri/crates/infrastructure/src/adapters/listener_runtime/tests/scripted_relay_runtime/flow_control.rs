//! 生产 `ListenerRuntimeAdapter` 装配下的双向调度、half-close 与方向超时证据。

use std::{sync::Arc, time::Duration};

use intercept_proxy_application::{
    EventHub, EventSubscription, ListenerRuntimePort, SocketDiagnosticDirection,
    SocketDiagnosticStage, UiEventPayload,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Barrier,
};

use super::support::{
    read_to_end_bounded, start_scripted_runtime, start_scripted_runtime_from_listener,
};
use super::{SCRIPT, listener};

const TEST_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn simultaneous_opposite_direction_frames_cross_the_real_scripted_relay() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = super::reserve_port().await;
    let barrier = Arc::new(Barrier::new(2));
    let upstream_barrier = Arc::clone(&barrier);
    let mut upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        upstream_barrier.wait().await;
        stream.write_all(&[2, 22]).await.unwrap();
        let mut request = [0_u8; 2];
        stream.read_exact(&mut request).await.unwrap();
        request
    });
    let (runtime, configured) =
        start_scripted_runtime("opposite-direction", SCRIPT, listener_port, upstream_port).await;
    let exchange = tokio::time::timeout(TEST_TIMEOUT, async {
        let mut client = TcpStream::connect(("127.0.0.1", listener_port))
            .await
            .unwrap();
        barrier.wait().await;
        client.write_all(&[2, 11]).await.unwrap();
        let mut response = [0_u8; 2];
        client.read_exact(&mut response).await.unwrap();
        let request = (&mut upstream_task).await.unwrap();
        (request, response)
    })
    .await;
    if exchange.is_err() {
        upstream_task.abort();
        let _ = upstream_task.await;
    }
    runtime.stop(configured.id).await.unwrap();
    let (request, response) = exchange.expect("both relay directions must finish within the bound");

    assert_eq!(request, [161, 11]);
    assert_eq!(response, [209, 22]);
}

#[tokio::test]
async fn client_half_close_keeps_the_opposite_direction_open_until_upstream_eof() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = super::reserve_port().await;
    let mut upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = Vec::new();
        stream.read_to_end(&mut request).await.unwrap();
        assert_eq!(request, [161, 11]);
        stream.write_all(&[2, 22]).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let (runtime, configured) =
        start_scripted_runtime("client-half-close", SCRIPT, listener_port, upstream_port).await;
    let exchange = tokio::time::timeout(TEST_TIMEOUT, async {
        let mut client = TcpStream::connect(("127.0.0.1", listener_port))
            .await
            .unwrap();
        client.write_all(&[2, 11]).await.unwrap();
        client.shutdown().await.unwrap();
        let response = read_to_end_bounded(&mut client).await;
        (&mut upstream_task).await.unwrap();
        response
    })
    .await;
    if exchange.is_err() {
        upstream_task.abort();
        let _ = upstream_task.await;
    }
    runtime.stop(configured.id).await.unwrap();
    assert_eq!(
        exchange.expect("half-close relay must finish within the bound"),
        [209, 22]
    );
}

#[tokio::test]
async fn silent_client_reports_client_to_server_read_timeout() {
    assert_directional_timeout(SocketDiagnosticDirection::ClientToServer).await;
}

#[tokio::test]
async fn silent_upstream_reports_server_to_client_read_timeout() {
    assert_directional_timeout(SocketDiagnosticDirection::ServerToClient).await;
}

async fn assert_directional_timeout(expected: SocketDiagnosticDirection) {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = super::reserve_port().await;
    let events = Arc::new(EventHub::default());
    let mut subscription = events.subscribe_default(0).unwrap();
    let mut configured = listener(listener_port, upstream_port);
    configured.read_timeout_ms = 100;
    let (runtime, configured, _) = start_scripted_runtime_from_listener(
        "runtime-matrix",
        SCRIPT,
        configured,
        Some(Arc::clone(&events)),
        false,
    )
    .await;
    let client = tokio::time::timeout(
        TEST_TIMEOUT,
        TcpStream::connect(("127.0.0.1", listener_port)),
    )
    .await
    .expect("relay listener must accept within the bound")
    .unwrap();
    let (server, _) = tokio::time::timeout(TEST_TIMEOUT, upstream.accept())
        .await
        .expect("relay must connect upstream within the bound")
        .unwrap();

    let (mut client_read, mut client_write) = client.into_split();
    let (mut server_read, mut server_write) = server.into_split();
    let refresher = tokio::spawn(async move {
        loop {
            let result = match expected {
                SocketDiagnosticDirection::ClientToServer => {
                    async {
                        server_write.write_all(&[2, 22]).await?;
                        let mut response = [0_u8; 2];
                        client_read.read_exact(&mut response).await?;
                        std::io::Result::Ok(())
                    }
                    .await
                }
                SocketDiagnosticDirection::ServerToClient => {
                    async {
                        client_write.write_all(&[2, 11]).await?;
                        let mut request = [0_u8; 2];
                        server_read.read_exact(&mut request).await?;
                        std::io::Result::Ok(())
                    }
                    .await
                }
                _ => unreachable!(),
            };
            if result.is_err() {
                return;
            }
        }
    });

    // 先持续驱动非被测方向，再等待 opened 事件；否则慢 runner 可能在测试保活
    // 启动前让两个方向同时达到 100ms 超时，使首个终止方向不确定。
    if !wait_for_opened(&mut subscription).await {
        refresher.abort();
        let _ = refresher.await;
        runtime.stop(configured.id).await.unwrap();
        panic!("relay did not publish its opened evidence within the bound");
    }

    let observed = wait_for_directional_timeout(&mut subscription, expected).await;
    refresher.abort();
    let _ = refresher.await;
    runtime.stop(configured.id).await.unwrap();
    assert!(
        observed,
        "silent direction must emit the exact timeout stage"
    );
}

async fn wait_for_opened(subscription: &mut EventSubscription) -> bool {
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            let Some(event) = subscription.live.recv().await else {
                return false;
            };
            if matches!(
                event.payload,
                UiEventPayload::DiagnosticLogAdded(ref entry)
                    if entry.summary == "Socket 上游连接已建立"
            ) {
                return true;
            }
        }
    })
    .await
    .unwrap_or(false)
}

async fn wait_for_directional_timeout(
    subscription: &mut EventSubscription,
    expected: SocketDiagnosticDirection,
) -> bool {
    tokio::time::timeout(TEST_TIMEOUT, async {
        loop {
            let Some(event) = subscription.live.recv().await else {
                return false;
            };
            let UiEventPayload::DiagnosticLogAdded(entry) = event.payload else {
                continue;
            };
            if entry.summary != "Socket 连接已失败" {
                continue;
            }
            return entry.socket_context.is_some_and(|context| {
                context.stage == SocketDiagnosticStage::RelayRead
                    && context.direction == Some(expected)
            });
        }
    })
    .await
    .unwrap_or(false)
}
