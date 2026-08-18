//! T25 `LocalResponder` 从 Listener 入口到真实 TCP 回包的运行链测试。
//!
//! 这些测试只通过 `ListenerRuntimeAdapter::start` 进入生产装配，不直接调用协议协调器，
//! 因而能够同时证明协议包快照、LocalResponder factory、Frame Pump 和线路写出的真实接线。

use std::sync::Arc;

use intercept_proxy_domain::{
    DocumentAction, DocumentCondition, DocumentFieldName, DocumentValue, SocketDirection,
    SocketDocumentRuleDefinition, SocketDocumentRuleId,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Barrier,
};

use super::*;
use support::*;

#[derive(Clone, Copy, Debug)]
struct LocalState {
    decode: bool,
    encode: bool,
}

#[tokio::test]
async fn four_valid_decode_encode_states_use_the_real_local_response_chain() {
    for state in [
        LocalState {
            decode: false,
            encode: false,
        },
        LocalState {
            decode: true,
            encode: false,
        },
        LocalState {
            decode: false,
            encode: true,
        },
        LocalState {
            decode: true,
            encode: true,
        },
    ] {
        let id = format!("local-matrix-{}-{}", state.decode, state.encode);
        let listener_port = reserve_port().await;
        let listener = local_listener(&id, listener_port, state.decode, state.encode);
        let rules = if state.encode {
            vec![set_amount_rule(&listener, state.decode, 42)]
        } else {
            Vec::new()
        };
        let workspace = workspace(listener.clone(), rules);
        let (runtime, captures) = start_local_runtime_with_capture(
            &id,
            BASIC_SCHEMA,
            BASIC_SCRIPT,
            workspace,
            &listener,
            Arc::new(intercept_proxy_application::EventHub::default()),
        )
        .await;

        let response = request_once(listener_port, &[2, 11]).await;
        let expected = if state.encode {
            vec![209, 42]
        } else {
            vec![2, 11]
        };
        assert_eq!(response, expected, "failed state: {state:?}");
        let row = captures::wait_for_rows(&captures, 1).await.rows.remove(0);
        let detail = captures.get_detail(row.capture_id).unwrap().record;
        let intercept_proxy_application::SocketCapturePayload::LocalExchange(exchange) =
            detail.payload
        else {
            panic!("expected LocalExchange")
        };
        assert_eq!(exchange.request_document.is_some(), state.decode);
        assert_eq!(exchange.request_display.is_some(), state.decode);
        assert_eq!(exchange.request_origin, [2, 11]);
        assert_eq!(exchange.written_response, expected);
        assert_eq!(exchange.response_encode_enabled, state.encode);
        assert_eq!(
            exchange.response_write_kind,
            if state.encode {
                intercept_proxy_application::SocketWriteKind::Encoded
            } else {
                intercept_proxy_application::SocketWriteKind::Original
            }
        );
        assert!(matches!(
            exchange.response_display,
            intercept_proxy_application::SocketDisplayResult::UntrustedHtml { .. }
        ));

        runtime.stop(listener.id).await.unwrap();
    }
}

#[tokio::test]
async fn no_rule_encode_off_echoes_and_encode_on_uses_the_unmodified_document() {
    for (encode, expected) in [(false, vec![2, 17]), (true, vec![209, 17])] {
        let id = format!("local-no-rule-{encode}");
        let listener_port = reserve_port().await;
        let listener = local_listener(&id, listener_port, true, encode);
        let workspace = workspace(listener.clone(), Vec::new());
        let runtime =
            start_local_runtime(&id, BASIC_SCHEMA, BASIC_SCRIPT, workspace, &listener).await;

        assert_eq!(request_once(listener_port, &[2, 17]).await, expected);
        runtime.stop(listener.id).await.unwrap();
    }
}

#[tokio::test]
async fn iso8583_style_rule_clones_request_fields_and_sets_response_fields() {
    let id = "local-iso8583";
    let listener_port = reserve_port().await;
    let listener = local_listener(id, listener_port, true, true);
    let rule = SocketDocumentRuleDefinition::new(
        SocketDocumentRuleId::new(),
        true,
        10,
        1,
        listener.id,
        package_ref(id),
        1,
        SocketDirection::Downstream,
        vec![DocumentCondition::Equals {
            field: DocumentFieldName::new("mti").unwrap(),
            value: DocumentValue::String("0200".into()),
        }],
        vec![
            DocumentAction::SetField {
                field: DocumentFieldName::new("mti").unwrap(),
                value: DocumentValue::String("0210".into()),
            },
            DocumentAction::SetField {
                field: DocumentFieldName::new("response_code").unwrap(),
                value: DocumentValue::String("00".into()),
            },
        ],
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
    let listener = local_listener(id, listener_port, true, true);
    let amount = DocumentFieldName::new("amount").unwrap();
    let rule = SocketDocumentRuleDefinition::new(
        SocketDocumentRuleId::new(),
        true,
        10,
        1,
        listener.id,
        package_ref(id),
        1,
        SocketDirection::Downstream,
        vec![DocumentCondition::Equals {
            field: amount.clone(),
            value: DocumentValue::Int(11),
        }],
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
    let listener = local_listener(id, listener_port, true, true);
    let workspace = workspace(listener.clone(), Vec::new());
    let runtime = start_local_runtime(id, BASIC_SCHEMA, BASIC_SCRIPT, workspace, &listener).await;

    let mut client = TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    client.write_all(&[2, 7, 2, 8, 2, 9]).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, [209, 7, 209, 8, 209, 9]);

    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn concurrent_connections_keep_request_and_response_documents_isolated() {
    let id = "local-connection-isolation";
    let listener_port = reserve_port().await;
    let listener = local_listener(id, listener_port, true, true);
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
        assert_eq!(response, [209, amount]);
    }
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn local_exchange_skips_nonexistent_upstream_and_downstream_input_stages() {
    let id = "local-direction-guards";
    let port = reserve_port().await;
    let listener = local_listener(id, port, true, true);
    let script = r#"
fn frame(reader, context) {
    if context.direction() != "upstream" { throw "downstream frame must not run"; }
    if reader.available() < 2 { framing::need_more(2) }
    else { framing::complete(2) }
}
fn decode(origin, context) {
    if context.direction() != "upstream" { throw "downstream decode must not run"; }
    let result = document::create();
    result.set("amount", origin[1]);
    result
}
fn encode(origin, document, context) {
    if context.direction() != "downstream" { throw "upstream encode must not run"; }
    let result = blob(2, 0);
    result[0] = 209;
    result[1] = document.get("amount");
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

    assert_eq!(request_once(port, &[2, 61]).await, [209, 61]);
    let status = runtime.statuses().await.unwrap().pop().unwrap();
    assert_eq!(status.client_to_server_bytes, 0);
    assert_eq!(status.server_to_client_bytes, 2);
    runtime.stop(listener.id).await.unwrap();
}

fn set_amount_rule(
    listener: &ProxyListener,
    require_request_match: bool,
    amount: i64,
) -> SocketDocumentRuleDefinition {
    let conditions = if require_request_match {
        vec![DocumentCondition::Equals {
            field: DocumentFieldName::new("amount").unwrap(),
            value: DocumentValue::Int(11),
        }]
    } else {
        // Domain 用空条件列表表达无条件恒匹配；不存在协议专属的 Always 哨兵类型。
        Vec::new()
    };
    SocketDocumentRuleDefinition::new(
        SocketDocumentRuleId::new(),
        true,
        10,
        1,
        listener.id,
        package_ref(listener_package_id(listener)),
        1,
        SocketDirection::Downstream,
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
