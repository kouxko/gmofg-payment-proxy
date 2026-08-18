//! Scripted Relay 的旁路 Display 与 Frame/Decode/Encode 失败原子性。

use std::sync::Arc;

use intercept_proxy_application::{
    SocketCapturePayload, SocketDisplayFallbackReason, SocketDisplayResult,
};

use super::{support::*, *};

const DISPLAY_FAILURE_SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 2 { framing::need_more(2) } else { framing::complete(2) }
}
fn decode(origin, context) {
    let result = document::create();
    result.set("amount", origin[1]);
    result
}
fn encode(origin, document, context) {
    let result = origin;
    result[0] = if context.direction() == "upstream" { 161 } else { 209 };
    result[1] = document.get("amount");
    result
}
fn display(document, context) { throw "display failed"; }
"#;

#[tokio::test]
async fn display_failure_keeps_wire_open_for_the_next_frame() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = reserve_port().await;
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut requests = [0_u8; 4];
        stream.read_exact(&mut requests).await.unwrap();
        assert_eq!(requests, [161, 11, 161, 12]);
        stream.write_all(&[2, 21, 2, 22]).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let (runtime, listener, captures) = start_scripted_runtime_with_capture(
        "display-failure",
        DISPLAY_FAILURE_SCRIPT,
        listener_port,
        upstream_port,
    )
    .await;

    let mut client = TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    client.write_all(&[2, 11, 2, 12]).await.unwrap();
    let mut responses = [0_u8; 4];
    client.read_exact(&mut responses).await.unwrap();

    assert_eq!(responses, [209, 21, 209, 22]);
    assert!(read_to_end_bounded(&mut client).await.is_empty());
    upstream_task.await.unwrap();
    assert_four_display_fallbacks(&captures).await;
    runtime.stop(listener.id).await.unwrap();
}

async fn assert_four_display_fallbacks(
    captures: &Arc<crate::adapters::SocketCaptureRepositoryAdapter>,
) {
    let page = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let page = captures.query(&captures::query()).unwrap();
            if page.total == 4 {
                break page;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        let page = captures.query(&captures::query()).unwrap();
        let observed = page
            .rows
            .into_iter()
            .map(|row| {
                let detail = captures.get_detail(row.capture_id).unwrap();
                let SocketCapturePayload::RelayFrame(frame) = detail.record.payload else {
                    panic!("expected relay frame capture")
                };
                format!("{:?}:{:?}", frame.direction, frame.origin)
            })
            .collect::<Vec<_>>();
        panic!("four committed relay captures should persist; observed {observed:?}")
    });

    for row in page.rows {
        let detail = captures.get_detail(row.capture_id).unwrap();
        let SocketCapturePayload::RelayFrame(frame) = detail.record.payload else {
            panic!("expected relay frame capture")
        };
        assert!(matches!(
            frame.display,
            SocketDisplayResult::HexFallback {
                reason: SocketDisplayFallbackReason::EntryPointFailed,
                ..
            }
        ));
    }
}

#[tokio::test]
async fn frame_failure_writes_zero_bytes_before_closing() {
    assert_upstream_stage_failure_writes_nothing(
        "frame-failure",
        r#"
fn frame(reader, context) { throw "frame failed"; }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
fn display(document, context) { "ok" }
"#,
    )
    .await;
}

#[tokio::test]
async fn frame_operation_limit_writes_zero_bytes_before_closing() {
    assert_upstream_stage_failure_writes_nothing(
        "frame-operation-limit",
        r#"
fn frame(reader, context) { while true {} }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
fn display(document, context) { "ok" }
"#,
    )
    .await;
}

#[tokio::test]
async fn decode_failure_writes_zero_bytes_before_closing() {
    assert_upstream_stage_failure_writes_nothing(
        "decode-failure",
        r#"
fn frame(reader, context) { framing::complete(2) }
fn decode(origin, context) { throw "decode failed"; }
fn encode(origin, document, context) { origin }
fn display(document, context) { "ok" }
"#,
    )
    .await;
}

#[tokio::test]
async fn encode_failure_writes_zero_bytes_before_closing() {
    assert_upstream_stage_failure_writes_nothing(
        "encode-failure",
        r#"
fn frame(reader, context) { framing::complete(2) }
fn decode(origin, context) {
    let result = document::create();
    result.set("amount", origin[1]);
    result
}
fn encode(origin, document, context) { throw "encode failed"; }
fn display(document, context) { "ok" }
"#,
    )
    .await;
}

async fn assert_upstream_stage_failure_writes_nothing(id: &str, script: &str) {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = reserve_port().await;
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        read_to_end_bounded(&mut stream).await
    });
    let (runtime, listener) =
        start_scripted_runtime(id, script, listener_port, upstream_port).await;

    let mut client = TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    client.write_all(&[2, 33]).await.unwrap();

    assert!(read_to_end_bounded(&mut client).await.is_empty());
    assert!(upstream_task.await.unwrap().is_empty());
    runtime.stop(listener.id).await.unwrap();
}
