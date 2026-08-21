use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Serialize, Serializer, ser::Error as _};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::{Peer, next_request, raw_pair, registered, registered_with_config, test_config};
use crate::adapters::external_packages::{
    ExternalPackageClient, ExternalPackageConnectionConfig, ExternalPackageConnectionError,
    ExternalPackageFatalProtocolError, ExternalPackageRemoteError,
    actor::{recent_ids::RecentRequestIds, response::parse_response},
};

mod config;
mod transport;

async fn begin_registration(
    config: ExternalPackageConnectionConfig,
) -> (
    tokio::task::JoinHandle<
        Result<
            (
                intercept_proxy_domain::ExternalPackageRegistration,
                ExternalPackageClient,
            ),
            ExternalPackageConnectionError,
        >,
    >,
    Peer,
) {
    let (actor_socket, peer) = raw_pair().await;
    let connecting = tokio::spawn(ExternalPackageClient::connect(actor_socket, 21, config));
    (connecting, peer)
}

async fn answer_registration(peer: &mut Peer, response: Value) {
    let request = next_request(peer).await;
    assert_eq!(request["method"], "package.register");
    peer.send(Message::Text(response.to_string().into()))
        .await
        .expect("registration response");
}

#[test]
fn recent_request_ids_ignore_duplicates_and_evict_oldest_entry() {
    let mut ids = RecentRequestIds::new(2);
    ids.insert("first".into());
    ids.insert("first".into());
    ids.insert("second".into());
    ids.insert("third".into());

    assert!(!ids.contains("first"));
    assert!(ids.contains("second"));
    assert!(ids.contains("third"));
}

#[test]
fn recent_request_ids_remove_known_entry_only_once() {
    let mut ids = RecentRequestIds::new(1);
    ids.insert("request".into());

    assert!(ids.remove("request"));
    assert!(!ids.remove("request"));
}

#[test]
fn response_parser_rejects_every_invalid_envelope_shape() {
    for text in [
        "not-json",
        "[]",
        r#"{"jsonrpc":"1.0","id":"a","result":{}}"#,
        r#"{"jsonrpc":"2.0","result":{}}"#,
        r#"{"jsonrpc":"2.0","id":"a","result":{},"error":{}}"#,
        r#"{"jsonrpc":"2.0","id":"a"}"#,
        r#"{"jsonrpc":"2.0","id":"a","result":{},"extra":true}"#,
        r#"{"jsonrpc":"2.0","id":"a","error":{"code":"bad","message":"x"}}"#,
    ] {
        assert!(parse_response(text).is_err(), "accepted {text}");
    }
}

#[tokio::test]
async fn registration_remote_error_is_reported_without_closing_as_protocol_error() {
    let (connecting, mut peer) = begin_registration(test_config(1)).await;
    answer_registration(
        &mut peer,
        json!({
            "jsonrpc":"2.0", "id":"register-21",
            "error":{"code":-32002,"message":"registration rejected"}
        }),
    )
    .await;

    assert!(matches!(
        connecting.await.expect("join").expect_err("remote error"),
        ExternalPackageConnectionError::Remote { .. }
    ));
}

#[tokio::test]
async fn registration_rejects_response_with_wrong_id() {
    let (connecting, mut peer) = begin_registration(test_config(1)).await;
    answer_registration(&mut peer, json!({"jsonrpc":"2.0","id":"wrong","result":{}})).await;

    assert!(matches!(
        connecting.await.expect("join").expect_err("wrong id"),
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::WrongResponseId)
    ));
}

#[tokio::test]
async fn registration_rejects_invalid_json() {
    let (connecting, mut peer) = begin_registration(test_config(1)).await;
    let _ = next_request(&mut peer).await;
    peer.send(Message::Text("{".into()))
        .await
        .expect("invalid JSON");

    assert!(matches!(
        connecting.await.expect("join").expect_err("invalid JSON"),
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidJson)
    ));
}

#[tokio::test]
async fn registration_rejects_binary_message() {
    let (connecting, mut peer) = begin_registration(test_config(1)).await;
    let _ = next_request(&mut peer).await;
    peer.send(Message::Binary(vec![1].into()))
        .await
        .expect("binary");

    assert!(matches!(
        connecting.await.expect("join").expect_err("binary"),
        ExternalPackageConnectionError::Fatal(
            ExternalPackageFatalProtocolError::RegistrationProtocolViolation
        )
    ));
}

#[tokio::test]
async fn registration_rejects_oversized_text_message() {
    let config = ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(10),
        Duration::from_secs(30),
        1,
        1024,
        32,
        1024,
        128,
    );
    let (connecting, mut peer) = begin_registration(config).await;
    let _ = next_request(&mut peer).await;
    peer.send(Message::Text("x".repeat(33).into()))
        .await
        .expect("oversized text");

    assert!(matches!(
        connecting.await.expect("join").expect_err("oversized"),
        ExternalPackageConnectionError::MessageTooLarge { .. }
    ));
}

#[tokio::test]
async fn registration_close_frame_reports_disconnect() {
    let (connecting, mut peer) = begin_registration(test_config(1)).await;
    let _ = next_request(&mut peer).await;
    peer.close(None).await.expect("close");

    assert!(matches!(
        connecting.await.expect("join").expect_err("disconnect"),
        ExternalPackageConnectionError::Disconnected
    ));
}

#[tokio::test(start_paused = true)]
async fn registration_heartbeat_disconnects_when_no_pong_arrives() {
    let config = ExternalPackageConnectionConfig::new(
        Duration::from_mins(1),
        Duration::from_secs(5),
        Duration::from_secs(5),
        Duration::from_secs(5),
        1,
        1024,
        1024,
        1024,
        128,
    );
    let (connecting, mut peer) = begin_registration(config).await;
    let _ = next_request(&mut peer).await;
    tokio::time::advance(Duration::from_secs(5)).await;

    assert!(matches!(
        connecting
            .await
            .expect("join")
            .expect_err("heartbeat timeout"),
        ExternalPackageConnectionError::Disconnected
    ));
    let _ = peer.next().await;
}

#[tokio::test(start_paused = true)]
async fn registration_heartbeat_sends_ping_before_timeout() {
    let config = ExternalPackageConnectionConfig::new(
        Duration::from_mins(1),
        Duration::from_secs(5),
        Duration::from_secs(5),
        Duration::from_secs(15),
        1,
        1024,
        1024,
        1024,
        128,
    );
    let (connecting, mut peer) = begin_registration(config).await;
    let _ = next_request(&mut peer).await;
    tokio::time::advance(Duration::from_secs(5)).await;
    let Message::Ping(payload) = peer.next().await.expect("ping").expect("valid ping") else {
        panic!("expected heartbeat ping")
    };
    peer.send(Message::Pong(payload)).await.expect("pong");
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":"register-21","result":super::valid_registration()})
            .to_string()
            .into(),
    ))
    .await
    .expect("registration response");

    connecting.await.expect("join").expect("registered");
}

#[tokio::test]
async fn registration_flushes_automatic_pong_before_accepting_result() {
    let (connecting, mut peer) = begin_registration(test_config(1)).await;
    let _ = next_request(&mut peer).await;
    peer.send(Message::Ping(vec![1, 2, 3].into()))
        .await
        .expect("ping");
    assert!(matches!(
        peer.next().await.expect("pong").expect("valid pong"),
        Message::Pong(payload) if payload.as_ref() == [1, 2, 3]
    ));
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":"register-21","result":super::valid_registration()})
            .to_string()
            .into(),
    ))
    .await
    .expect("registration response");

    connecting.await.expect("join").expect("registered");
}

#[tokio::test]
async fn invalid_registration_result_closes_connection_as_fatal() {
    let (connecting, mut peer) = begin_registration(test_config(1)).await;
    answer_registration(
        &mut peer,
        json!({"jsonrpc":"2.0","id":"register-21","result":{"api":1}}),
    )
    .await;

    assert!(matches!(
        connecting
            .await
            .expect("join")
            .expect_err("invalid registration"),
        ExternalPackageConnectionError::Fatal(
            ExternalPackageFatalProtocolError::InvalidRegistration
        )
    ));
}

#[tokio::test]
async fn registered_client_exposes_generation_and_message_limit() {
    let (client, _peer) = registered(1).await;

    assert_eq!(client.generation(), 7);
    assert_eq!(client.max_rpc_message_bytes(), 1024 * 1024);
}

struct SerializationFailure;

impl Serialize for SerializationFailure {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(S::Error::custom("intentional serialization failure"))
    }
}

#[tokio::test]
async fn call_rejects_non_finite_json_parameter() {
    let (client, _peer) = registered(1).await;

    assert!(matches!(
        client
            .call::<_, Value>("hooks.upstream.decode", &SerializationFailure)
            .await,
        Err(ExternalPackageConnectionError::InvalidPayload(_))
    ));
}

#[tokio::test]
async fn call_rejects_request_larger_than_rpc_limit_without_sending_it() {
    let config = ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(10),
        Duration::from_secs(30),
        1,
        1024,
        1024,
        64,
        32,
    );
    let (client, mut peer) = registered_with_config(config).await;

    assert!(matches!(
        client
            .call::<_, Value>("hooks.upstream.decode", &json!({"payload":"x".repeat(128)}))
            .await,
        Err(ExternalPackageConnectionError::MessageTooLarge { .. })
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(10), peer.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn display_call_uses_its_smaller_response_limit() {
    let config = ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(10),
        Duration::from_secs(30),
        1,
        1024,
        1024,
        1024,
        64,
    );
    let (client, mut peer) = registered_with_config(config).await;
    let call = tokio::spawn(async move {
        client
            .call_display::<_, Value>("render_message", &json!({}))
            .await
    });
    let request = next_request(&mut peer).await;
    peer.send(Message::Text(
        json!({
            "jsonrpc":"2.0", "id":request["id"], "result":{"text":"x".repeat(128)}
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("large display response");

    assert!(matches!(
        call.await.expect("join").expect_err("display limit"),
        ExternalPackageConnectionError::MessageTooLarge { .. }
    ));
}

#[tokio::test]
async fn invalid_json_after_registration_closes_protocol() {
    let (mut client, mut peer) = registered(1).await;
    peer.send(Message::Text("{".into()))
        .await
        .expect("invalid JSON");

    assert!(matches!(
        client.wait_closed().await,
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidJson)
    ));
}

#[tokio::test]
async fn binary_message_after_registration_closes_protocol() {
    let (mut client, mut peer) = registered(1).await;
    peer.send(Message::Binary(vec![1].into()))
        .await
        .expect("binary");

    assert!(matches!(
        client.wait_closed().await,
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidResponse)
    ));
}

#[tokio::test]
async fn disconnect_fails_pending_call_with_disconnect_reason() {
    let (client, mut peer) = registered(1).await;
    let pending = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Value>("hooks.upstream.decode", &json!({}))
                .await
        }
    });
    let _ = next_request(&mut peer).await;
    peer.close(None).await.expect("close");

    assert!(matches!(
        pending.await.expect("join").expect_err("disconnected"),
        ExternalPackageConnectionError::Disconnected
    ));
}

#[tokio::test(start_paused = true)]
async fn actor_heartbeat_disconnects_when_registered_peer_stops_ponging() {
    let config = ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(5),
        Duration::from_secs(5),
        1,
        1024,
        1024,
        1024,
        128,
    );
    let (mut client, _peer) = registered_with_config(config).await;
    tokio::time::advance(Duration::from_secs(5)).await;

    assert!(matches!(
        client.wait_closed().await,
        ExternalPackageConnectionError::Disconnected
    ));
}

#[test]
fn every_connection_error_has_redacted_debug_and_stable_display() {
    let errors = [
        ExternalPackageConnectionError::Busy,
        ExternalPackageConnectionError::Timeout {
            request_id: "id".into(),
            method: "method".into(),
        },
        ExternalPackageConnectionError::Disconnected,
        ExternalPackageConnectionError::Remote {
            request_id: "id".into(),
            method: "method".into(),
            error: ExternalPackageRemoteError::new(
                -1,
                "remote".into(),
                Some(json!({"secret":"hidden"})),
            ),
        },
        ExternalPackageConnectionError::MessageTooLarge {
            actual_bytes: 2,
            limit_bytes: 1,
        },
        ExternalPackageConnectionError::InvalidPayload("secret".into()),
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidResponse),
        ExternalPackageConnectionError::Transport("secret".into()),
    ];

    for error in errors {
        assert!(!format!("{error:?}").contains("secret"));
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn remote_error_accessors_return_the_strict_wire_values() {
    let data = json!({"shape":"object"});
    let error = ExternalPackageRemoteError::new(-32_001, "rejected".into(), Some(data.clone()));

    assert_eq!(error.code(), -32_001);
    assert_eq!(error.message(), "rejected");
    assert_eq!(error.data(), Some(&data));
}
