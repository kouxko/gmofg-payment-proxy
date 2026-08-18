//! T25 `LocalResponder` 从 Listener 入口到真实 TCP 回包的运行链测试。
//!
//! 这些测试只通过 `ListenerRuntimeAdapter::start` 进入生产装配，不直接调用协议协调器，
//! 因而能够同时证明协议包快照、LocalResponder factory、Frame Pump 和线路写出的真实接线。

use std::sync::Arc;

use intercept_proxy_domain::{
    DocumentAction, DocumentCondition, DocumentFieldName, DocumentValue, ProtocolDirection,
    ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId, ProtocolRuleStage,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Barrier,
};

use super::*;
use support::*;

#[tokio::test]
async fn local_response_always_uses_the_full_protocol_chain() {
    let id = "local-full-chain";
    let listener_port = reserve_port().await;
    let listener = local_listener(id, listener_port);
    let workspace = workspace(
        listener.clone(),
        vec![set_amount_rule(&listener, false, 42)],
    );
    let (runtime, captures) = start_local_runtime_with_capture(
        id,
        BASIC_SCHEMA,
        BASIC_SCRIPT,
        workspace,
        &listener,
        Arc::new(intercept_proxy_application::EventHub::default()),
    )
    .await;

    let response = request_once(listener_port, &[2, 11]).await;
    assert_eq!(response, [209, 42]);
    let row = captures::wait_for_rows(&captures, 1).await.rows.remove(0);
    let detail = captures.get_detail(row.capture_id).unwrap().record;
    let intercept_proxy_application::SocketCapturePayload::LocalExchange(exchange) = detail.payload
    else {
        panic!("expected LocalExchange")
    };
    assert!(exchange.request_document.schema.version() > 0);
    assert_eq!(exchange.request_origin, [2, 11]);
    assert_eq!(exchange.written_response, [209, 42]);
    assert!(matches!(
        exchange.response_display,
        intercept_proxy_application::SocketDisplayResult::UntrustedHtml { .. }
    ));

    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn no_rule_encodes_the_empty_isolated_response_document() {
    let id = "local-no-rule";
    let listener_port = reserve_port().await;
    let listener = local_listener(id, listener_port);
    let workspace = workspace(listener.clone(), Vec::new());
    let runtime = start_local_runtime(id, BASIC_SCHEMA, BASIC_SCRIPT, workspace, &listener).await;

    assert_eq!(request_once(listener_port, &[2, 17]).await, [209, 0]);
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn iso8583_style_rule_builds_the_isolated_response_document() {
    let id = "local-iso8583";
    let listener_port = reserve_port().await;
    let listener = local_listener(id, listener_port);
    let rule = ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::new(),
        true,
        10,
        1,
        listener.id,
        package_ref(id),
        1,
        ProtocolDirection::Downstream,
        Vec::new(),
        vec![DocumentAction::SetField {
            field: DocumentFieldName::new("message").unwrap(),
            value: DocumentValue::Blob(b"02101234560000100000".to_vec()),
        }],
    )
    .unwrap();
    let workspace = workspace(listener.clone(), vec![rule]);
    let runtime = start_local_runtime(id, ISO_SCHEMA, ISO_SCRIPT, workspace, &listener).await;

    let request = b"020012345600001000";
    let response = request_once(listener_port, request).await;
    assert_eq!(response, b"02101234560000100000");

    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn matching_rule_applies_set_clear_and_set_actions_in_declared_order() {
    let id = "local-action-order";
    let listener_port = reserve_port().await;
    let listener = local_listener(id, listener_port);
    let amount = DocumentFieldName::new("amount").unwrap();
    let rule = ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::new(),
        true,
        10,
        1,
        listener.id,
        package_ref(id),
        1,
        ProtocolDirection::Downstream,
        Vec::new(),
        vec![
            DocumentAction::SetField {
                field: amount.clone(),
                value: DocumentValue::Int(99),
            },
            DocumentAction::ClearDocument,
            DocumentAction::SetField {
                field: amount,
                value: DocumentValue::Int(33),
            },
        ],
    )
    .unwrap();
    let workspace = workspace(listener.clone(), vec![rule]);
    let runtime = start_local_runtime(id, BASIC_SCHEMA, BASIC_SCRIPT, workspace, &listener).await;

    assert_eq!(request_once(listener_port, &[2, 11]).await, [209, 33]);
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn pipelined_requests_are_replied_once_each_in_fifo_order() {
    let id = "local-fifo";
    let listener_port = reserve_port().await;
    let listener = local_listener(id, listener_port);
    let workspace = workspace(listener.clone(), Vec::new());
    let runtime = start_local_runtime(id, BASIC_SCHEMA, BASIC_SCRIPT, workspace, &listener).await;

    let mut client = TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    client.write_all(&[2, 7, 2, 8, 2, 9]).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, [209, 0, 209, 0, 209, 0]);

    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn concurrent_connections_do_not_leak_request_fields_into_response_documents() {
    let id = "local-connection-isolation";
    let listener_port = reserve_port().await;
    let listener = local_listener(id, listener_port);
    let workspace = workspace(listener.clone(), Vec::new());
    let runtime = start_local_runtime(id, BASIC_SCHEMA, BASIC_SCRIPT, workspace, &listener).await;
    let barrier = Arc::new(Barrier::new(3));
    let mut clients = Vec::new();

    for amount in [13_u8, 29] {
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
        assert_eq!(
            response,
            [209, 0],
            "request amount {amount} must stay upstream"
        );
    }
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn local_exchange_uses_receive_then_downstream_send_without_upstream_io() {
    let id = "local-direction-guards";
    let port = reserve_port().await;
    let listener = local_listener(id, port);
    let script = r#"
fn frame(reader, context) {
    if reader.available() < 2 { framing::need_more(2) }
    else { framing::complete(2) }
}
fn decode(origin, context) {
    let result = document::create();
    result.set("amount", origin[1].to_int());
    result
}
fn encode(origin, document, context) {
    let result = blob(2, 0);
    result[0] = 209;
    result[1] = if document.has("amount") { document.get("amount") } else { 0 };
    result
}
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

    assert_eq!(request_once(port, &[2, 61]).await, [209, 0]);
    let status = runtime.statuses().await.unwrap().pop().unwrap();
    assert_eq!(status.client_to_server_bytes, 0);
    assert_eq!(status.server_to_client_bytes, 2);
    runtime.stop(listener.id).await.unwrap();
}

fn set_amount_rule(
    listener: &ProxyListener,
    require_request_match: bool,
    amount: i64,
) -> ProtocolDocumentRuleDefinition {
    let conditions = if require_request_match {
        vec![DocumentCondition::Equals {
            field: DocumentFieldName::new("amount").unwrap(),
            value: DocumentValue::Int(11),
        }]
    } else {
        // Domain 用空条件列表表达无条件恒匹配；不存在协议专属的 Always 哨兵类型。
        Vec::new()
    };
    ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::new(),
        true,
        10,
        1,
        listener.id,
        package_ref(listener_package_id(listener)),
        1,
        ProtocolDirection::Downstream,
        conditions,
        vec![DocumentAction::SetField {
            field: DocumentFieldName::new("amount").unwrap(),
            value: DocumentValue::Int(amount),
        }],
    )
    .unwrap()
}

#[path = "local_responder_runtime/captures.rs"]
mod captures;
#[path = "local_responder_runtime/failures.rs"]
mod failures;
#[path = "local_responder_runtime/request_parsed.rs"]
mod request_parsed;
#[path = "local_responder_runtime/support.rs"]
mod support;
