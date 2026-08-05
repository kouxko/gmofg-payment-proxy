use std::{sync::Arc, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tun2proxy::CancellationToken;

use super::{RecordingProtection, ipv4_connect_request, no_auth};
use crate::routing::ProxyRouteTable;

#[tokio::test]
async fn server_limits_concurrent_clients_and_reuses_released_permit() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let server = tokio::spawn(super::super::run_server_with_limits(
        listener,
        RecordingProtection::default(),
        Arc::new(ProxyRouteTable::default()),
        server_cancellation,
        0,
        super::super::ServerLimits {
            max_clients: 1,
            handshake_timeout: Duration::from_secs(2),
            ..super::super::ServerLimits::default()
        },
    ));

    let first = TcpStream::connect(address).await.unwrap();
    let mut second = TcpStream::connect(address).await.unwrap();
    second.write_all(&[5, 1, 0]).await.unwrap();
    let mut reply = [0_u8; 2];
    assert!(
        timeout(Duration::from_millis(75), second.read_exact(&mut reply))
            .await
            .is_err(),
        "第一个会话占用许可时，第二个会话不得进入握手"
    );

    drop(first);
    timeout(Duration::from_secs(1), second.read_exact(&mut reply))
        .await
        .expect("释放许可后第二个会话应被处理")
        .expect("第二个会话握手成功");
    assert_eq!(reply, [5, 0]);

    cancellation.cancel();
    timeout(Duration::from_secs(1), server)
        .await
        .expect("取消后服务应回收所有子任务")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn stalled_handshake_times_out_without_stopping_server() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let server = tokio::spawn(super::super::run_server_with_limits(
        listener,
        RecordingProtection::default(),
        Arc::new(ProxyRouteTable::default()),
        server_cancellation,
        0,
        super::super::ServerLimits {
            max_clients: 1,
            handshake_timeout: Duration::from_millis(50),
            ..super::super::ServerLimits::default()
        },
    ));

    let mut stalled = TcpStream::connect(address).await.unwrap();
    let mut byte = [0_u8; 1];
    let read = timeout(Duration::from_secs(1), stalled.read(&mut byte))
        .await
        .expect("握手超时后连接应关闭")
        .unwrap();
    assert_eq!(read, 0);

    let mut next = TcpStream::connect(address).await.unwrap();
    next.write_all(&[5, 1, 0]).await.unwrap();
    let mut reply = [0_u8; 2];
    next.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [5, 0]);

    cancellation.cancel();
    timeout(Duration::from_secs(1), server)
        .await
        .expect("服务取消应及时完成")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn idle_relay_times_out_and_releases_the_session() {
    let upstream = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (_stream, _) = upstream.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let server = tokio::spawn(super::super::run_server_with_limits(
        listener,
        RecordingProtection::default(),
        Arc::new(ProxyRouteTable::default()),
        cancellation.clone(),
        0,
        super::super::ServerLimits {
            idle_timeout: Duration::from_millis(50),
            ..super::super::ServerLimits::default()
        },
    ));

    let mut client = TcpStream::connect(address).await.unwrap();
    no_auth(&mut client).await;
    client
        .write_all(&ipv4_connect_request(upstream_address))
        .await
        .unwrap();
    let mut reply = [0_u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], super::super::protocol::SUCCEEDED);

    let mut byte = [0_u8; 1];
    let read = timeout(Duration::from_secs(1), client.read(&mut byte))
        .await
        .expect("空闲超时后 relay 应关闭")
        .unwrap();
    assert_eq!(read, 0);

    cancellation.cancel();
    timeout(Duration::from_secs(1), server)
        .await
        .expect("服务取消应及时完成")
        .unwrap()
        .unwrap();
    upstream_task.abort();
}
