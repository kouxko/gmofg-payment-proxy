use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

use super::{
    DataCommand, ExternalPackageClient, ExternalPackageConnectionError,
    ExternalPackageFatalProtocolError,
};
use crate::adapters::external_packages::tests::{test_config, valid_registration};

impl ExternalPackageClient {
    fn saturate_data_queue(&self) {
        let mut sequence = 0_u64;
        loop {
            match self
                .commands
                .try_send(DataCommand::Cancel(format!("saturated-{sequence}")))
            {
                Ok(()) => sequence = sequence.wrapping_add(1),
                Err(
                    tokio::sync::mpsc::error::TrySendError::Full(_)
                    | tokio::sync::mpsc::error::TrySendError::Closed(_),
                ) => return,
            }
        }
    }
}

async fn registered_client() -> (
    ExternalPackageClient,
    WebSocketStream<tokio::io::DuplexStream>,
) {
    let (actor_io, peer_io) = tokio::io::duplex(32 * 1024);
    let actor = WebSocketStream::from_raw_socket(actor_io, Role::Server, None).await;
    let mut peer = WebSocketStream::from_raw_socket(peer_io, Role::Client, None).await;
    let connecting = tokio::spawn(ExternalPackageClient::connect(actor, 7, test_config(1)));
    let request = peer
        .next()
        .await
        .expect("registration request")
        .expect("valid registration request")
        .into_text()
        .expect("text registration request");
    let request: Value = serde_json::from_str(&request).expect("registration JSON");
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":valid_registration()})
            .to_string()
            .into(),
    ))
    .await
    .expect("registration response");
    let (_, client) = connecting.await.expect("join").expect("registered");
    (client, peer)
}

#[tokio::test]
async fn malformed_typed_response_is_fatal_while_data_queue_is_saturated() {
    let (client, mut peer) = registered_client().await;
    let mut monitor = client.clone();
    let call = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Vec<String>>("hooks.upstream.decode", &json!({}))
                .await
        }
    });
    let request = peer
        .next()
        .await
        .expect("business request")
        .expect("valid business request")
        .into_text()
        .expect("text business request");
    let request: Value = serde_json::from_str(&request).expect("business JSON");
    client.saturate_data_queue();
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":{"wrong":"shape"}})
            .to_string()
            .into(),
    ))
    .await
    .expect("malformed typed response");

    assert!(matches!(
        call.await.expect("join").expect_err("typed result"),
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidResponse)
    ));
    assert!(matches!(
        monitor.wait_closed().await,
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidResponse)
    ));
}

#[tokio::test]
async fn disconnect_closes_actor_while_data_queue_is_saturated() {
    let (client, _peer) = registered_client().await;
    let mut monitor = client.clone();
    client.saturate_data_queue();

    client.disconnect().await;

    assert!(matches!(
        monitor.wait_closed().await,
        ExternalPackageConnectionError::Disconnected
    ));
}

#[tokio::test]
async fn client_debug_reports_generation_and_available_capacity() {
    let (client, _peer) = registered_client().await;

    let debug = format!("{client:?}");

    assert!(debug.contains("generation: 7"));
    assert!(debug.contains("available_permits: 1"));
}

#[tokio::test]
async fn client_reports_configured_rpc_timeout() {
    let (client, _peer) = registered_client().await;

    assert_eq!(client.rpc_timeout(), std::time::Duration::from_secs(5));
}

#[tokio::test]
async fn client_reports_configured_logical_frame_limit() {
    let (client, _peer) = registered_client().await;

    assert_eq!(client.max_logical_frame_bytes(), 1024 * 1024);
}
