use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::DuplexStream;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role, protocol::WebSocketConfig},
};

use super::{Peer, next_request, raw_pair, test_config, valid_registration};
use crate::adapters::external_packages::{
    ExternalPackageClient, ExternalPackageConnectionConfig, ExternalPackageConnectionError,
};

async fn raw_pair_with_capacity(capacity: usize) -> (WebSocketStream<DuplexStream>, Peer) {
    let (actor_io, peer_io) = tokio::io::duplex(capacity);
    let actor = WebSocketStream::from_raw_socket(actor_io, Role::Server, None).await;
    let peer = WebSocketStream::from_raw_socket(peer_io, Role::Client, None).await;
    (actor, peer)
}

async fn raw_pair_with_actor_message_limit(limit: usize) -> (WebSocketStream<DuplexStream>, Peer) {
    let (actor_io, peer_io) = tokio::io::duplex(32 * 1024);
    let websocket_config = WebSocketConfig::default()
        .max_message_size(Some(limit))
        .max_frame_size(Some(limit));
    let actor =
        WebSocketStream::from_raw_socket(actor_io, Role::Server, Some(websocket_config)).await;
    let peer = WebSocketStream::from_raw_socket(peer_io, Role::Client, None).await;
    (actor, peer)
}

#[tokio::test]
async fn registration_preserves_a_ping_prefetched_in_the_same_transport_read() {
    let (actor_socket, mut peer) = raw_pair().await;
    let connecting = tokio::spawn(ExternalPackageClient::connect(
        actor_socket,
        7,
        test_config(1),
    ));
    let _ = next_request(&mut peer).await;
    peer.feed(Message::Text(
        json!({"jsonrpc":"2.0","id":"register-7","result":valid_registration()})
            .to_string()
            .into(),
    ))
    .await
    .expect("queue registration response");
    peer.feed(Message::Ping(vec![1, 2, 3].into()))
        .await
        .expect("queue adjacent ping");
    peer.flush().await.expect("flush adjacent frames together");
    let (_registration, client) = connecting.await.expect("join").expect("registered");

    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(100), peer.next())
            .await
            .expect("prefetched ping must be processed")
            .expect("pong frame")
            .expect("valid pong"),
        Message::Pong(payload) if payload.as_ref() == [1, 2, 3]
    ));
    client.disconnect().await;
}

#[tokio::test(start_paused = true)]
async fn blocked_websocket_writer_closes_actor_at_rpc_deadline() {
    let config = ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(10),
        Duration::from_secs(30),
        2,
        16 * 1024,
        16 * 1024,
        16 * 1024,
        128,
    );
    let (actor_socket, mut peer) = raw_pair_with_capacity(256).await;
    let connecting = tokio::spawn(ExternalPackageClient::connect(actor_socket, 7, config));
    let request = next_request(&mut peer).await;
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":valid_registration()})
            .to_string()
            .into(),
    ))
    .await
    .expect("registration response");
    let (_registration, mut client) = connecting.await.expect("join").expect("registered");
    let call = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Value>(
                    "hooks.upstream.decode",
                    &json!({"payload":"x".repeat(8 * 1024)}),
                )
                .await
        }
    });
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(5)).await;
    assert!(matches!(
        call.await.expect("join").expect_err("deadline"),
        ExternalPackageConnectionError::Timeout { .. }
    ));
    let closed = tokio::spawn(async move { client.wait_closed().await });
    // The actor may dequeue the call only after the caller's own deadline fires. Advance one
    // complete writer deadline from that observable point rather than relying on task order.
    tokio::time::advance(Duration::from_secs(6)).await;

    assert!(matches!(
        closed.await.expect("join"),
        ExternalPackageConnectionError::Disconnected
    ));
}

#[tokio::test]
async fn oversized_wire_response_fails_pending_call_as_message_too_large() {
    let (actor_socket, mut peer) = raw_pair_with_actor_message_limit(2 * 1024).await;
    let connecting = tokio::spawn(ExternalPackageClient::connect(
        actor_socket,
        7,
        ExternalPackageConnectionConfig::new(
            Duration::from_secs(30),
            Duration::from_secs(5),
            Duration::from_secs(10),
            Duration::from_secs(30),
            1,
            16 * 1024,
            2 * 1024,
            16 * 1024,
            128,
        ),
    ));
    let registration_request = next_request(&mut peer).await;
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":registration_request["id"],"result":valid_registration()})
            .to_string()
            .into(),
    ))
    .await
    .expect("registration response");
    let (_registration, client) = connecting.await.expect("join").expect("registered");
    let mut monitor = client.clone();
    let call = tokio::spawn(async move {
        client
            .call::<_, Value>("hooks.upstream.decode", &json!({}))
            .await
    });
    let request = next_request(&mut peer).await;
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":{"payload":"x".repeat(4 * 1024)}})
            .to_string()
            .into(),
    ))
    .await
    .expect("oversized response");

    assert!(matches!(
        call.await.expect("join").expect_err("wire limit"),
        ExternalPackageConnectionError::MessageTooLarge {
            limit_bytes: 2_048,
            ..
        }
    ));
    assert!(matches!(
        monitor.wait_closed().await,
        ExternalPackageConnectionError::MessageTooLarge {
            limit_bytes: 2_048,
            ..
        }
    ));
}

#[tokio::test]
async fn outbound_request_over_wire_limit_fails_call_without_sending_or_disconnect() {
    let config = ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(10),
        Duration::from_secs(30),
        1,
        16 * 1024,
        2 * 1024,
        16 * 1024,
        128,
    );
    let (client, mut peer) = super::registered_with_config(config).await;

    assert!(matches!(
        client
            .call::<_, Value>(
                "hooks.upstream.decode",
                &json!({"payload":"x".repeat(4 * 1024)}),
            )
            .await,
        Err(ExternalPackageConnectionError::MessageTooLarge {
            limit_bytes: 2_048,
            ..
        })
    ));
    let small = tokio::spawn(async move {
        client
            .call::<_, Value>("hooks.upstream.decode", &json!({"small":true}))
            .await
    });
    let request = next_request(&mut peer).await;
    assert_eq!(request["params"], json!({"small":true}));
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":{"ok":true}})
            .to_string()
            .into(),
    ))
    .await
    .expect("small response");

    assert_eq!(
        small.await.expect("join").expect("small call"),
        json!({"ok":true})
    );
}
