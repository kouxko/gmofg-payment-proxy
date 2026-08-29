//! 连续 Frame 与并发连接不得共享 Document 或连接级执行状态。

use std::sync::Arc;

use tokio::sync::Barrier;

use super::{support::*, *};

const ISOLATION_SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 2 { framing::need_more(2) } else { framing::complete(2) }
}

fn decode(origin, context) {
    let result = document::create();
    if origin[1] != 0 { result.set("/amount", origin[1]); }
    result
}

fn encode(origin, document, context) {
    let result = origin;
    result[0] = if context.direction() == "upstream" { 161 } else { 209 };
    result[1] = if document.has("/amount") { document.get("/amount") } else { 0 };
    result
}

fn display(document, context) { "<p>ok</p>" }
"#;

#[tokio::test]
async fn consecutive_frames_on_one_connection_use_fresh_document_and_call_scope() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = reserve_port().await;
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut received = [0_u8; 4];
        stream.read_exact(&mut received[..2]).await.unwrap();
        stream.write_all(&[2, 17]).await.unwrap();
        stream.read_exact(&mut received[2..]).await.unwrap();
        stream.write_all(&[2, 10]).await.unwrap();
        stream.shutdown().await.unwrap();
        received
    });
    let (runtime, listener) = start_scripted_runtime(
        "fresh-frame-state",
        ISOLATION_SCRIPT,
        listener_port,
        upstream_port,
    )
    .await;

    let mut client = TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    client.write_all(&[2, 7]).await.unwrap();
    let mut response = [0_u8; 2];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(response, [209, 17]);
    client.write_all(&[2, 0]).await.unwrap();
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(response, [209, 10]);
    client.shutdown().await.unwrap();

    // 第二个 Decode 故意不设置 amount：若复用上一 Frame 的 Document，会错误保留 7；
    // 同一脚本调用中由当前 origin 得出的值也必须来自新的 Rhai Scope，不能串到下一 Frame。
    assert_eq!(upstream_task.await.unwrap(), [161, 7, 161, 0]);
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn concurrent_connections_keep_documents_and_outputs_isolated() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = reserve_port().await;
    let upstream_task = tokio::spawn(async move {
        let mut handlers = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = upstream.accept().await.unwrap();
            handlers.push(tokio::spawn(async move {
                let mut request = [0_u8; 2];
                stream.read_exact(&mut request).await.unwrap();
                assert_eq!(request[0], 161);
                stream.write_all(&[2, request[1] + 10]).await.unwrap();
                stream.shutdown().await.unwrap();
            }));
        }
        for handler in handlers {
            handler.await.unwrap();
        }
    });
    let (runtime, listener) = start_scripted_runtime(
        "connection-isolation",
        ISOLATION_SCRIPT,
        listener_port,
        upstream_port,
    )
    .await;
    let barrier = Arc::new(Barrier::new(3));
    let mut clients = Vec::new();
    for amount in [11_u8, 29] {
        let barrier = Arc::clone(&barrier);
        clients.push(tokio::spawn(async move {
            let mut client = TcpStream::connect(("127.0.0.1", listener_port))
                .await
                .unwrap();
            barrier.wait().await;
            client.write_all(&[2, amount]).await.unwrap();
            let mut response = [0_u8; 2];
            client.read_exact(&mut response).await.unwrap();
            (amount, response)
        }));
    }
    barrier.wait().await;

    for client in clients {
        let (amount, response) = client.await.unwrap();
        assert_eq!(response, [209, amount + 10]);
    }
    upstream_task.await.unwrap();
    runtime.stop(listener.id).await.unwrap();
}
