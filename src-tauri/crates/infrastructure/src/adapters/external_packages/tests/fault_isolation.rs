use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::Message;

use super::{Peer, next_request, raw_pair, test_config, valid_registration};
use crate::adapters::external_packages::{
    ExternalPackageClient, ExternalPackageConnectionConfig, ExternalPackageConnectionError,
    ExternalPackageFatalProtocolError,
};

async fn registered_package_with_config(
    package_id: &str,
    generation: u64,
    config: ExternalPackageConnectionConfig,
) -> (ExternalPackageClient, Peer) {
    let (actor_socket, mut peer) = raw_pair().await;
    let connecting = tokio::spawn(ExternalPackageClient::connect(
        actor_socket,
        generation,
        config,
    ));
    let Message::Text(request) = peer
        .next()
        .await
        .expect("registration request")
        .expect("valid frame")
    else {
        panic!("registration must be text")
    };
    let request: Value = serde_json::from_str(&request).expect("valid request");
    assert_eq!(request["id"], format!("register-{generation}"));
    let mut registration = valid_registration();
    registration["package"]["id"] = Value::String(package_id.to_owned());
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":registration})
            .to_string()
            .into(),
    ))
    .await
    .expect("registration response");
    let (registration, client) = connecting.await.expect("join").expect("registered");
    assert_eq!(registration.package().identity().id.as_str(), package_id);
    (client, peer)
}

#[tokio::test]
async fn malformed_wire_closes_only_the_offending_package_while_another_package_progresses() {
    let (mut malformed_client, mut malformed_peer) =
        registered_package_with_config("malformed-package", 101, test_config(1)).await;
    let (healthy_client, mut healthy_peer) =
        registered_package_with_config("healthy-package", 202, test_config(1)).await;
    let malformed_call = tokio::spawn({
        let client = malformed_client.clone();
        async move {
            client
                .call::<_, Value>("hooks.upstream.decode", &json!({"package":"malformed"}))
                .await
        }
    });
    let _ = next_request(&mut malformed_peer).await;
    malformed_peer
        .send(Message::Text("{".into()))
        .await
        .expect("malformed JSON response");
    assert!(matches!(
        malformed_call.await.expect("join").expect_err("fatal"),
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidJson)
    ));
    assert!(matches!(
        malformed_client.wait_closed().await,
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidJson)
    ));

    let healthy_call = tokio::spawn(async move {
        healthy_client
            .call::<_, Value>("hooks.upstream.decode", &json!({"package":"healthy"}))
            .await
    });
    let request = next_request(&mut healthy_peer).await;
    healthy_peer
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":request["id"],"result":{"ok":true}})
                .to_string()
                .into(),
        ))
        .await
        .expect("healthy response");
    assert_eq!(
        healthy_call.await.expect("join").expect("healthy progress"),
        json!({"ok":true})
    );
}

#[tokio::test]
async fn malformed_raw_websocket_frame_closes_only_the_offending_package_connection() {
    let (mut malformed_client, mut malformed_peer) =
        registered_package_with_config("raw-frame-package", 505, test_config(1)).await;
    let (healthy_client, mut healthy_peer) =
        registered_package_with_config("raw-frame-healthy", 606, test_config(1)).await;
    malformed_peer
        .get_mut()
        .write_all(&[0x83, 0x80, 0, 0, 0, 0])
        .await
        .expect("malformed raw WebSocket frame");
    malformed_peer.get_mut().flush().await.expect("flush");
    assert!(matches!(
        malformed_client.wait_closed().await,
        ExternalPackageConnectionError::Transport(_)
    ));

    let healthy_call = tokio::spawn(async move {
        healthy_client
            .call::<_, Value>("hooks.upstream.frame", &json!({"package":"healthy"}))
            .await
    });
    let request = next_request(&mut healthy_peer).await;
    healthy_peer
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":request["id"],"result":{"status":"need_more"}})
                .to_string()
                .into(),
        ))
        .await
        .expect("healthy response");
    assert_eq!(
        healthy_call.await.expect("join").expect("healthy progress"),
        json!({"status":"need_more"})
    );
}

#[tokio::test]
async fn stalled_package_does_not_consume_another_packages_in_flight_capacity() {
    let (stalled_client, mut stalled_peer) =
        registered_package_with_config("stalled-package", 303, test_config(1)).await;
    let (healthy_client, mut healthy_peer) =
        registered_package_with_config("independent-package", 404, test_config(1)).await;
    let stalled_call = tokio::spawn(async move {
        stalled_client
            .call::<_, Value>("hooks.upstream.decode", &json!({"package":"stalled"}))
            .await
    });
    let _ = next_request(&mut stalled_peer).await;
    let healthy_call = tokio::spawn(async move {
        healthy_client
            .call::<_, Value>("hooks.upstream.decode", &json!({"package":"independent"}))
            .await
    });
    let request = next_request(&mut healthy_peer).await;
    healthy_peer
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":request["id"],"result":{"ok":true}})
                .to_string()
                .into(),
        ))
        .await
        .expect("independent response");
    assert_eq!(
        healthy_call
            .await
            .expect("join")
            .expect("independent progress"),
        json!({"ok":true})
    );
    stalled_peer.close(None).await.expect("close stalled peer");
    assert!(matches!(
        stalled_call.await.expect("join").expect_err("disconnect"),
        ExternalPackageConnectionError::Disconnected
    ));
}
