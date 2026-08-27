//! 外部协议包从真实 WebSocket peer 到真实 Socket Listener 的端到端验收。
//!
//! 测试只从公开运行入口启动 `ExternalPackageServer` 与 `ListenerRuntimeAdapter`，并使用
//! localhost TCP client/upstream 验证线路结果；不直接调用 external processor。

use intercept_proxy_application::{ExternalPackageApplicationPort, ListenerRuntimePort};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

use external_package_runtime_support::*;

#[tokio::test]
async fn external_relay_handles_fragmentation_across_sequential_interactions() {
    let mut harness = ExternalRuntimeHarness::start().await;
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let listener_address = reserve_address().await;
    let listener = external_relay_listener(listener_address, upstream_address, &harness.package);
    let workspace = external_workspace(listener.clone(), Vec::new());

    harness.start_listener(workspace, listener.clone()).await;
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut first = [0_u8; 3];
        stream.read_exact(&mut first).await.unwrap();
        assert_eq!(first, [3, b'a', b'b']);
        stream.write_all(&[3, b'x', b'y']).await.unwrap();
        let mut second = [0_u8; 4];
        stream.read_exact(&mut second).await.unwrap();
        assert_eq!(second, [4, b'c', b'd', b'e']);
        stream.write_all(&[4, b'z', b'1', b'2']).await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let mut client = TcpStream::connect(listener_address).await.unwrap();
    client.write_all(&[3]).await.unwrap();
    harness.peer().wait_for_need_more().await;
    client.write_all(b"ab").await.unwrap();
    let mut first_response = [0_u8; 3];
    client.read_exact(&mut first_response).await.unwrap();
    assert_eq!(first_response, [3, b'x', b'y']);
    client.write_all(&[4, b'c', b'd', b'e']).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    timeout(TEST_TIMEOUT, client.read_to_end(&mut response))
        .await
        .expect("relay response deadline")
        .unwrap();

    assert_eq!(response, [4, b'z', b'1', b'2']);
    upstream_task.await.unwrap();
    assert_eq!(harness.peer().registration_count(), 1);
    harness.stop_listener(listener.id).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn external_local_server_echoes_one_payload_through_both_direction_hooks() {
    let mut harness = ExternalRuntimeHarness::start().await;
    let listener_address = reserve_address().await;
    let listener = external_local_listener(listener_address, &harness.package);
    let workspace = external_workspace(listener.clone(), Vec::new());
    harness.start_listener(workspace, listener.clone()).await;

    let mut client = TcpStream::connect(listener_address).await.unwrap();
    client.write_all(&[3, b'a', b'b']).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    timeout(TEST_TIMEOUT, client.read_to_end(&mut response))
        .await
        .expect("LocalResponder response deadline")
        .unwrap();

    assert_eq!(response, [3, b'a', b'b']);
    harness.stop_listener(listener.id).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn external_peer_disconnect_marks_package_offline_and_stops_exact_listener() {
    let mut harness = ExternalRuntimeHarness::start().await;
    let listener_address = reserve_address().await;
    let listener = external_local_listener(listener_address, &harness.package);
    let workspace = external_workspace(listener.clone(), Vec::new());
    harness.start_listener(workspace, listener.clone()).await;

    harness.disconnect_peer().await;
    harness.wait_until_offline().await;
    harness.wait_until_listener_stopped(listener.id).await;

    assert!(
        harness
            .registry
            .get(&harness.package)
            .await
            .unwrap()
            .is_some_and(|version| version.source.external_online() == Some(false))
    );
    assert!(harness.runtime.statuses().await.unwrap().is_empty());
    TcpListener::bind(listener_address)
        .await
        .expect("disconnect cleanup must release the exact listener port");
    harness.shutdown().await;
}

#[tokio::test]
async fn oversized_external_frame_boundary_closes_only_the_business_connection() {
    let mut harness = ExternalRuntimeHarness::start().await;
    let listener_address = reserve_address().await;
    let listener = external_local_listener(listener_address, &harness.package);
    let workspace = external_workspace(listener.clone(), Vec::new());
    harness.start_listener(workspace, listener.clone()).await;

    harness.peer().return_oversized_frame_boundary_once();
    let mut malformed = TcpStream::connect(listener_address).await.unwrap();
    malformed.write_all(&[3, b'a', b'b']).await.unwrap();
    malformed.shutdown().await.unwrap();
    let mut malformed_response = Vec::new();
    timeout(TEST_TIMEOUT, malformed.read_to_end(&mut malformed_response))
        .await
        .expect("malformed business connection closes")
        .unwrap();
    assert!(malformed_response.is_empty());
    assert!(
        harness
            .registry
            .get(&harness.package)
            .await
            .unwrap()
            .is_some_and(|version| version.source.external_online() == Some(true))
    );
    assert!(
        harness
            .runtime
            .statuses()
            .await
            .unwrap()
            .iter()
            .any(|status| status.listener_id == listener.id)
    );

    let mut healthy = TcpStream::connect(listener_address).await.unwrap();
    healthy.write_all(&[3, b'x', b'y']).await.unwrap();
    healthy.shutdown().await.unwrap();
    let mut healthy_response = Vec::new();
    timeout(TEST_TIMEOUT, healthy.read_to_end(&mut healthy_response))
        .await
        .expect("next business connection completes")
        .unwrap();
    assert_eq!(healthy_response, [3, b'x', b'y']);

    harness.stop_listener(listener.id).await;
    harness.shutdown().await;
}

#[path = "external_package_runtime/support.rs"]
mod external_package_runtime_support;
