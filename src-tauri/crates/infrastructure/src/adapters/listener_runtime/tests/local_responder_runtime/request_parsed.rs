//! `RequestParsed` 必须来自真实 request Decode，同时只向通用日志复制有界形状元数据。

use std::sync::Arc;

use intercept_proxy_application::{EventHub, UiEventPayload};

use super::{support::*, *};

const LARGE_SCHEMA: &str = r#"
id = "local-large-request"
version = 1
title = "Large Request"

[[fields]]
name = "payload"
label = "Payload"
type = "blob"
"#;

const LARGE_SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 5000 { framing::need_more(5000) }
    else { framing::complete(5000) }
}
fn decode(origin, context) {
    let result = document::create();
    result.set("payload", origin);
    result
}
fn encode(origin, document, context) { origin }
fn display(document, context) { "unused" }
"#;

#[tokio::test]
async fn request_parsed_reports_bounded_origin_and_document_shape_from_real_decode() {
    let id = "local-request-parsed-large";
    let port = reserve_port().await;
    let listener = local_listener(id, port);
    let events = Arc::new(EventHub::new(32));
    let runtime = start_local_runtime_with_events(
        id,
        LARGE_SCHEMA,
        LARGE_SCRIPT,
        workspace(listener.clone(), Vec::new()),
        &listener,
        Arc::clone(&events),
    )
    .await;
    let request = vec![0xAB; 5_000];

    assert_eq!(request_once(port, &request).await, request);
    let detail = request_parsed_detail(&events);
    assert!(detail.contains("request：5000 字节"), "{detail}");
    assert!(detail.contains("原始预览：4096 字节"), "{detail}");
    assert!(detail.contains("Schema local-large-request@1"), "{detail}");
    // 通用 EventHub 只接收形状元数据，绝不能复制预览 payload。
    assert!(!detail.contains("abababab"), "{detail}");

    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn request_parsed_reports_the_required_upstream_document_schema() {
    let id = "local-request-parsed-schema";
    let port = reserve_port().await;
    let listener = local_listener(id, port);
    let events = Arc::new(EventHub::new(32));
    let runtime = start_local_runtime_with_events(
        id,
        BASIC_SCHEMA,
        BASIC_SCRIPT,
        workspace(listener.clone(), Vec::new()),
        &listener,
        Arc::clone(&events),
    )
    .await;

    assert_eq!(request_once(port, &[2, 77]).await, [209, 0]);
    let detail = request_parsed_detail(&events);
    assert!(detail.contains("Schema local-basic@1"), "{detail}");

    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn retained_request_diagnostics_enforce_the_256_event_bound() {
    let id = "local-request-parsed-capacity";
    let port = reserve_port().await;
    let listener = local_listener(id, port);
    // EventHub 与 Proxy observer 是两个独立边界。这里给 EventHub 足够容量，专门观察
    // Socket observer 自己的 256 条淘汰计数，而不是触发 UI replay 淘汰。
    let events = Arc::new(EventHub::new(1_024));
    let runtime = start_local_runtime_with_events(
        id,
        BASIC_SCHEMA,
        BASIC_SCRIPT,
        workspace(listener.clone(), Vec::new()),
        &listener,
        events,
    )
    .await;
    let request_count = 260_usize;
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let requests = [2_u8, 1].repeat(request_count);
    client.write_all(&requests).await.unwrap();
    client.shutdown().await.unwrap();
    let mut responses = Vec::new();
    client.read_to_end(&mut responses).await.unwrap();
    assert_eq!(responses, [209_u8, 0].repeat(request_count));

    let status = runtime.statuses().await.unwrap().pop().unwrap();
    assert!(
        status.retained_diagnostic_evictions > 0,
        "260 RequestParsed plus lifecycle events must exceed the 256-event observer bound"
    );
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn retained_request_diagnostics_enforce_the_one_mib_logical_byte_bound() {
    let id = "local-request-parsed-bytes";
    let port = reserve_port().await;
    let listener = local_listener(id, port);
    let events = Arc::new(EventHub::new(1_024));
    let runtime = start_local_runtime_with_events(
        id,
        LARGE_SCHEMA,
        LARGE_SCRIPT,
        workspace(listener.clone(), Vec::new()),
        &listener,
        events,
    )
    .await;
    // 每个事件保留 4 KiB origin 预览和约 10 KiB 的 Blob hex Document 预览。
    // 80 条仍远少于 256 条，因此发生淘汰只能来自 1 MiB 逻辑字节门禁。
    let request_count = 80_usize;
    let one_request = vec![0xCD; 5_000];
    let requests = one_request.repeat(request_count);
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(&requests).await.unwrap();
    client.shutdown().await.unwrap();
    let mut responses = Vec::new();
    client.read_to_end(&mut responses).await.unwrap();
    assert_eq!(responses, requests);

    let status = runtime.statuses().await.unwrap().pop().unwrap();
    assert!(
        status.retained_diagnostic_evictions > 0,
        "80 large previews are below the count bound but must exceed one MiB"
    );
    runtime.stop(listener.id).await.unwrap();
}

fn request_parsed_detail(events: &EventHub) -> String {
    events
        .replay_after(0)
        .events
        .into_iter()
        .find_map(|event| match event.payload {
            UiEventPayload::DiagnosticLogAdded(entry)
                if entry.summary == "Socket 本地请求已解析" =>
            {
                entry.detail
            }
            _ => None,
        })
        .expect("real LocalResponder request must publish RequestParsed")
}
