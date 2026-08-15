//! `LocalResponder` 所有 pre-write 失败必须关闭当前连接且零输出。

use super::{support::*, *};
use tokio::net::TcpListener;

#[tokio::test]
async fn frame_decode_and_encode_failures_write_zero_response_bytes() {
    for (id, script, decode, encode) in [
        (
            "local-frame-failure",
            r#"
fn frame(reader, context) { throw "frame failed"; }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
fn display(document, context) { "ok" }
"#,
            false,
            false,
        ),
        (
            "local-decode-failure",
            r#"
fn frame(reader, context) { framing::complete(2) }
fn decode(origin, context) { throw "decode failed"; }
fn encode(origin, document, context) { origin }
fn display(document, context) { "ok" }
"#,
            true,
            false,
        ),
        (
            "local-encode-failure",
            r#"
fn frame(reader, context) { framing::complete(2) }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { throw "encode failed"; }
fn display(document, context) { "ok" }
"#,
            false,
            true,
        ),
    ] {
        assert_pre_write_failure(id, script, decode, encode, &[2, 33]).await;
    }
}

#[tokio::test]
async fn empty_oversize_and_timed_out_encode_write_zero_response_bytes() {
    for (id, encode_body) in [
        ("local-empty-response", "blob()"),
        ("local-oversize-response", "blob(1048577, 1)"),
        ("local-encode-timeout", "while true {}; origin"),
    ] {
        let script = format!(
            r#"
fn frame(reader, context) {{ framing::complete(2) }}
fn decode(origin, context) {{ document::create() }}
fn encode(origin, document, context) {{ {encode_body} }}
fn display(document, context) {{ "ok" }}
"#,
        );
        assert_pre_write_failure(id, &script, false, true, &[2, 44]).await;
    }
}

#[tokio::test]
async fn oversize_frame_request_and_truncated_request_write_zero_response_bytes() {
    assert_pre_write_failure(
        "local-oversize-request",
        r#"
fn frame(reader, context) { framing::need_more(1048577) }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
fn display(document, context) { "ok" }
"#,
        false,
        false,
        &[1],
    )
    .await;

    assert_pre_write_failure(
        "local-truncated-request",
        BASIC_SCRIPT,
        false,
        false,
        &[4, 1],
    )
    .await;
}

#[tokio::test]
async fn empty_connection_produces_no_spontaneous_response() {
    let id = "local-empty-input";
    let port = reserve_port().await;
    let listener = local_listener(id, port, true, true);
    let runtime = start_local_runtime(
        id,
        BASIC_SCHEMA,
        BASIC_SCRIPT,
        workspace(listener.clone(), Vec::new()),
        &listener,
    )
    .await;

    assert!(request_once(port, &[]).await.is_empty());
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn downstream_display_failure_happens_after_committed_response_and_is_non_fatal() {
    let id = "local-display-failure";
    let port = reserve_port().await;
    let listener = local_listener(id, port, true, true);
    let script = BASIC_SCRIPT.replace(
        "fn display(document, context) { \"<p>local response</p>\" }",
        "fn display(document, context) { throw \"display failed\"; }",
    );
    let (runtime, captures) = start_local_runtime_with_capture(
        id,
        BASIC_SCHEMA,
        &script,
        workspace(listener.clone(), Vec::new()),
        &listener,
        Arc::new(intercept_proxy_application::EventHub::default()),
    )
    .await;

    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(&[2, 51, 2, 52]).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, [209, 51, 209, 52]);
    let page = super::captures::wait_for_rows(&captures, 2).await;
    for row in page.rows {
        let detail = captures.get_detail(row.capture_id).unwrap().record;
        let intercept_proxy_application::SocketCapturePayload::LocalExchange(exchange) =
            detail.payload
        else {
            panic!("expected LocalExchange")
        };
        assert!(matches!(
            exchange.response_display,
            intercept_proxy_application::SocketDisplayResult::HexFallback {
                reason: intercept_proxy_application::SocketDisplayFallbackReason::EntryPointFailed,
                ..
            }
        ));
    }

    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn stopping_during_decode_cancels_the_blocking_exchange_and_releases_the_listener() {
    let id = "local-stop-during-decode";
    let port = reserve_port().await;
    let listener = local_listener(id, port, true, false);
    let script = r#"
fn frame(reader, context) { framing::complete(2) }
fn decode(origin, context) { while true {} }
fn encode(origin, document, context) { origin }
fn display(document, context) { "ok" }
"#;
    let runtime = start_local_runtime(
        id,
        BASIC_SCHEMA,
        script,
        workspace(listener.clone(), Vec::new()),
        &listener,
    )
    .await;
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(&[2, 88]).await.unwrap();
    tokio::task::yield_now().await;

    tokio::time::timeout(std::time::Duration::from_secs(2), runtime.stop(listener.id))
        .await
        .expect("stop must not wait for the Rhai wall-time deadline")
        .unwrap();
    let mut response = Vec::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client.read_to_end(&mut response),
    )
    .await
    .expect("cancelled connection must close");
    if let Err(error) = read {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }
    assert!(response.is_empty());
    TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("cancelled LocalResponder must release its listener port");
}

async fn assert_pre_write_failure(
    id: &str,
    script: &str,
    decode: bool,
    encode: bool,
    request: &[u8],
) {
    let port = reserve_port().await;
    let listener = local_listener(id, port, decode, encode);
    let runtime = start_local_runtime(
        id,
        BASIC_SCHEMA,
        script,
        workspace(listener.clone(), Vec::new()),
        &listener,
    )
    .await;

    assert!(
        request_once(port, request).await.is_empty(),
        "failure case {id}"
    );
    runtime.stop(listener.id).await.unwrap();
}
