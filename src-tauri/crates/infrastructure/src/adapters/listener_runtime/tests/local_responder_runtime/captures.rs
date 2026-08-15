//! `LocalResponder` 正式 capture 只在 response 全量写出后产生。

use std::{sync::Arc, time::Duration};

use intercept_proxy_application::{
    EventHub, PageRequest, SocketCaptureDocumentValue, SocketCaptureInteger, SocketCapturePayload,
    SocketCaptureQuery, SocketCaptureSort, SocketDisplayFallbackReason, SocketDisplayResult,
    SocketWriteKind, SortDirection,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::*;

pub(super) fn query() -> SocketCaptureQuery {
    SocketCaptureQuery {
        workspace_id: None,
        listener_id: None,
        session_id: None,
        connection_id: None,
        package: None,
        direction: None,
        kind: None,
        occurred_from: None,
        occurred_to: None,
        sort: SocketCaptureSort::OccurredAt,
        direction_sort: SortDirection::Asc,
        page: PageRequest {
            page: 1,
            page_size: 20,
        },
    }
}

pub(super) async fn wait_for_rows(
    repository: &crate::adapters::SocketCaptureRepositoryAdapter,
    expected: usize,
) -> intercept_proxy_application::SocketCapturePageViewModel {
    for _ in 0..100 {
        let page = repository.query(&query()).expect("capture query");
        if page.rows.len() == expected {
            return page;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("capture drain did not reach {expected} rows");
}

#[tokio::test]
async fn partial_request_has_no_capture_then_commit_persists_one_exact_exchange() {
    let id = "local-capture-commit";
    let port = reserve_port().await;
    let listener = local_listener(id, port, true, true);
    let rule_id = SocketDocumentRuleId::new();
    let rule = SocketDocumentRuleDefinition::new(
        rule_id,
        true,
        10,
        1,
        listener.id,
        package_ref(id),
        1,
        SocketDirection::Downstream,
        Vec::new(),
        vec![DocumentAction::SetField {
            field: DocumentFieldName::new("amount").unwrap(),
            value: DocumentValue::Int(42),
        }],
    )
    .unwrap();
    let events = Arc::new(EventHub::default());
    let (runtime, captures) = start_local_runtime_with_capture(
        id,
        BASIC_SCHEMA,
        BASIC_SCRIPT,
        workspace(listener.clone(), vec![rule]),
        &listener,
        Arc::clone(&events),
    )
    .await;

    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(&[2]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(captures.query(&query()).unwrap().total, 0);

    client.write_all(&[11]).await.unwrap();
    let mut response = [0_u8; 2];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(response, [209, 42]);

    let row = wait_for_rows(&captures, 1).await.rows.remove(0);
    let detail = captures.get_detail(row.capture_id).unwrap().record;
    let SocketCapturePayload::LocalExchange(exchange) = detail.payload else {
        panic!("expected LocalExchange")
    };
    let exchange_id = exchange.exchange_id;
    assert_eq!(exchange.request_origin, [2, 11]);
    assert_eq!(
        exchange.request_document.unwrap().get("amount").unwrap(),
        &SocketCaptureDocumentValue::Int(SocketCaptureInteger::from_i64(11))
    );
    assert_eq!(
        exchange.response_document.get("amount").unwrap(),
        &SocketCaptureDocumentValue::Int(SocketCaptureInteger::from_i64(42))
    );
    assert_eq!(exchange.matched_downstream_rule_ids, [rule_id]);
    assert_eq!(exchange.written_response, [209, 42]);
    assert_eq!(exchange.response_write_kind, SocketWriteKind::Encoded);
    assert_eq!(
        exchange.response_display,
        SocketDisplayResult::UntrustedHtml {
            html: "<p>local response</p>".to_owned()
        }
    );
    let replay = events.replay_after(0);
    let completed = replay
        .events
        .iter()
        .find(|event| {
            matches!(
                event.payload,
                intercept_proxy_application::UiEventPayload::SocketCaptureCompleted(_)
            )
        })
        .expect("completed event");
    assert_eq!(completed.occurred_at, row.completed_at);
    let request_detail = replay
        .events
        .iter()
        .find_map(|event| match &event.payload {
            intercept_proxy_application::UiEventPayload::DiagnosticLogAdded(entry)
                if entry.summary == "Socket 本地请求已解析" =>
            {
                entry.detail.as_deref()
            }
            _ => None,
        })
        .expect("RequestParsed diagnostic");
    assert!(request_detail.contains(&exchange_id.to_string()));
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn decode_off_capture_has_no_request_document_and_echo_fallback_is_explicit() {
    let id = "local-capture-decode-off";
    let port = reserve_port().await;
    let listener = local_listener(id, port, false, false);
    let (runtime, captures) = start_local_runtime_with_capture(
        id,
        BASIC_SCHEMA,
        BASIC_SCRIPT,
        workspace(listener.clone(), Vec::new()),
        &listener,
        Arc::new(EventHub::default()),
    )
    .await;

    assert_eq!(request_once(port, &[2, 9]).await, [2, 9]);
    let row = wait_for_rows(&captures, 1).await.rows.remove(0);
    let detail = captures.get_detail(row.capture_id).unwrap().record;
    let SocketCapturePayload::LocalExchange(exchange) = detail.payload else {
        panic!("expected LocalExchange")
    };
    assert!(exchange.request_document.is_none());
    assert_eq!(exchange.response_write_kind, SocketWriteKind::Original);
    assert_eq!(exchange.written_response, [2, 9]);
    assert_eq!(
        exchange.response_display,
        SocketDisplayResult::HexFallback {
            reason: SocketDisplayFallbackReason::EncodeDisabled,
            diagnostic: None,
        }
    );
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn decode_failure_writes_and_publishes_no_completed_exchange() {
    let id = "local-capture-failure";
    let port = reserve_port().await;
    let listener = local_listener(id, port, true, true);
    let script = BASIC_SCRIPT.replace(
        "let result = document::create();",
        "throw \"decode failed\"; let result = document::create();",
    );
    let events = Arc::new(EventHub::default());
    let (runtime, captures) = start_local_runtime_with_capture(
        id,
        BASIC_SCHEMA,
        &script,
        workspace(listener.clone(), Vec::new()),
        &listener,
        Arc::clone(&events),
    )
    .await;

    assert!(request_once(port, &[2, 9]).await.is_empty());
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(captures.query(&query()).unwrap().total, 0);
    assert!(!events.replay_after(0).events.iter().any(|event| matches!(
        event.payload,
        intercept_proxy_application::UiEventPayload::SocketCaptureCompleted(_)
    )));
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn clear_after_output_commit_cannot_revive_a_blocked_local_capture() {
    let id = "local-capture-clear-race";
    let port = reserve_port().await;
    let listener = local_listener(id, port, true, true);
    let workspace = workspace(listener.clone(), Vec::new());
    let workspace_id = workspace.id;
    let (runtime, captures) = start_local_runtime_with_capture(
        id,
        BASIC_SCHEMA,
        BASIC_SCRIPT,
        workspace,
        &listener,
        Arc::new(EventHub::default()),
    )
    .await;
    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    runtime.block_next_socket_capture_display_for_test(entered_sender, release_receiver);

    let request = tokio::spawn(async move { request_once(port, &[2, 9]).await });
    tokio::task::spawn_blocking(move || entered_receiver.recv_timeout(Duration::from_secs(2)))
        .await
        .unwrap()
        .expect("Display worker must stop after output_committed took its ticket");
    assert_eq!(captures.clear_completed(workspace_id).unwrap(), 0);
    release_sender.send(()).unwrap();
    assert_eq!(request.await.unwrap(), [209, 9]);

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(captures.query(&query()).unwrap().total, 0);
    runtime.stop(listener.id).await.unwrap();
}
