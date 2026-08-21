use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::DuplexStream;
use tokio_tungstenite::{
    WebSocketStream, client_async,
    tungstenite::{Message, protocol::Role},
};

use super::*;

mod backpressure;
mod coverage;

type Peer = WebSocketStream<DuplexStream>;

pub(super) fn valid_registration() -> Value {
    json!({
        "api": 1,
        "package": {"id":"vendor-dukpt-iso8583","name":"DUKPT ISO8583","version":"1.0.0","description":"external test package"},
        "document": {
            "upstream": {"schema":{"id":"dukpt-upstream","title":"Upstream","version":1,"fields":[{"name":"mti","label":"MTI","type":"string"}]},"display":"render_message"},
            "downstream": {"schema":{"id":"dukpt-downstream","title":"Downstream","version":1,"fields":[{"name":"response_code","label":"RC","type":"string"}]},"display":"render_message"}
        },
        "hooks": {
            "upstream":{"frame":"split_frame","decode":"decode","encode":"encode"},
            "downstream":{"frame":"split_frame","decode":"decode","encode":"encode"}
        }
    })
}

pub(super) fn test_config(max_in_flight: usize) -> ExternalPackageConnectionConfig {
    ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(10),
        Duration::from_secs(30),
        max_in_flight,
        1024 * 1024,
        1024 * 1024,
        1024 * 1024,
        128 * 1024,
    )
}

async fn raw_pair() -> (WebSocketStream<DuplexStream>, Peer) {
    let (actor_io, peer_io) = tokio::io::duplex(2 * 1024 * 1024);
    let actor = WebSocketStream::from_raw_socket(actor_io, Role::Server, None).await;
    let peer = WebSocketStream::from_raw_socket(peer_io, Role::Client, None).await;
    (actor, peer)
}

#[tokio::test]
async fn websocket_handshake_accepts_only_the_exact_packages_path() {
    let (server_io, client_io) = tokio::io::duplex(4096);
    let server = tokio::spawn(accept_packages_websocket(server_io, 1024));
    let (_client, response) = client_async("ws://localhost/packages", client_io)
        .await
        .expect("exact path accepted");
    assert_eq!(response.status(), 101);
    server.await.expect("server join").expect("server accepts");

    let (server_io, client_io) = tokio::io::duplex(4096);
    let server = tokio::spawn(accept_packages_websocket(server_io, 1024));
    assert!(
        client_async("ws://localhost/packages?token=x", client_io)
            .await
            .is_err()
    );
    assert!(server.await.expect("server join").is_err());
}

async fn registered(max_in_flight: usize) -> (ExternalPackageClient, Peer) {
    registered_with_config(test_config(max_in_flight)).await
}

async fn registered_with_config(
    config: ExternalPackageConnectionConfig,
) -> (ExternalPackageClient, Peer) {
    let (actor_socket, mut peer) = raw_pair().await;
    let connecting = tokio::spawn(ExternalPackageClient::connect(actor_socket, 7, config));
    let Message::Text(request) = peer
        .next()
        .await
        .expect("registration request")
        .expect("valid frame")
    else {
        panic!("registration must be text")
    };
    let request: Value = serde_json::from_str(&request).expect("valid request");
    assert_eq!(request["id"], "register-7");
    assert_eq!(request["method"], "package.register");
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":"register-7","result":valid_registration()})
            .to_string()
            .into(),
    ))
    .await
    .expect("registration response");
    let (registration, client) = connecting.await.expect("join").expect("registered");
    assert_eq!(
        registration.package().identity().id.as_str(),
        "vendor-dukpt-iso8583"
    );
    (client, peer)
}

async fn next_request(peer: &mut Peer) -> Value {
    loop {
        match peer
            .next()
            .await
            .expect("request frame")
            .expect("valid frame")
        {
            Message::Text(text) => return serde_json::from_str(&text).expect("request JSON"),
            Message::Ping(payload) => peer.send(Message::Pong(payload)).await.expect("pong"),
            other => panic!("unexpected frame: {other:?}"),
        }
    }
}

#[test]
fn defaults_match_the_external_package_contract() {
    let config = ExternalPackageConnectionConfig::default();
    assert_eq!(config.max_in_flight(), 256);
    assert_eq!(config.max_logical_frame_bytes(), 8 * 1024 * 1024);
    assert_eq!(config.max_registration_message_bytes(), 1024 * 1024);
    assert_eq!(config.max_display_message_bytes(), 128 * 1024);
}

#[tokio::test]
async fn remote_error_fails_only_its_call_and_debug_redacts_data() {
    let (client, mut peer) = registered(2).await;
    let first = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Value>("hooks.upstream.decode", &json!({"x":1}))
                .await
        }
    });
    let request = next_request(&mut peer).await;
    peer.send(Message::Text(json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32001,"message":"rejected","data":{"secret":"must-not-log"}}}).to_string().into()))
        .await.expect("error response");
    let error = first.await.expect("join").expect_err("remote error");
    assert!(matches!(
        error,
        ExternalPackageConnectionError::Remote { .. }
    ));
    assert!(!format!("{error:?}").contains("must-not-log"));

    let second = tokio::spawn(async move {
        client
            .call::<_, Value>("hooks.upstream.frame", &json!({"x":2}))
            .await
    });
    let request = next_request(&mut peer).await;
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":{"status":"need_more"}})
            .to_string()
            .into(),
    ))
    .await
    .expect("success response");
    assert_eq!(
        second.await.expect("join").expect("second call"),
        json!({"status":"need_more"})
    );
}

#[tokio::test]
async fn malformed_typed_result_marks_the_whole_protocol_fatal() {
    #[derive(Debug, serde::Deserialize)]
    struct StrictResult {
        #[allow(dead_code)]
        ok: bool,
    }

    let (mut client, mut peer) = registered(1).await;
    let call = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, StrictResult>("hooks.upstream.decode", &json!({}))
                .await
        }
    });
    let request = next_request(&mut peer).await;
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":{"wrong":true}})
            .to_string()
            .into(),
    ))
    .await
    .expect("malformed typed response");
    assert!(matches!(
        call.await.expect("join").expect_err("fatal result"),
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidResponse)
    ));
    assert!(matches!(
        client.wait_closed().await,
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidResponse)
    ));
}

#[tokio::test]
async fn wrong_id_and_duplicate_response_are_fatal() {
    let (client, mut peer) = registered(1).await;
    let call = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Value>("hooks.upstream.frame", &json!({}))
                .await
        }
    });
    let _ = next_request(&mut peer).await;
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":"g999-c1","result":{}})
            .to_string()
            .into(),
    ))
    .await
    .expect("wrong response");
    assert!(matches!(
        call.await.expect("join").expect_err("fatal"),
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::WrongResponseId)
    ));

    let (mut client, mut peer) = registered(1).await;
    let call = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Value>("hooks.upstream.frame", &json!({}))
                .await
        }
    });
    let request = next_request(&mut peer).await;
    let response = json!({"jsonrpc":"2.0","id":request["id"],"result":{}}).to_string();
    peer.send(Message::Text(response.clone().into()))
        .await
        .expect("response");
    call.await.expect("join").expect("success");
    peer.send(Message::Text(response.into()))
        .await
        .expect("duplicate");
    assert!(matches!(
        client.wait_closed().await,
        ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::DuplicateResponse)
    ));
}

#[tokio::test(start_paused = true)]
async fn timeout_releases_capacity_and_late_response_is_ignored() {
    let (client, mut peer) = registered(1).await;
    let first = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Value>("hooks.upstream.decode", &json!({}))
                .await
        }
    });
    let first_request = next_request(&mut peer).await;
    tokio::time::advance(Duration::from_secs(5)).await;
    assert!(matches!(
        first.await.expect("join").expect_err("timeout"),
        ExternalPackageConnectionError::Timeout { .. }
    ));
    tokio::task::yield_now().await;
    let second = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Value>("hooks.upstream.decode", &json!({}))
                .await
        }
    });
    let second_request = next_request(&mut peer).await;
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":first_request["id"],"result":{"late":true}})
            .to_string()
            .into(),
    ))
    .await
    .expect("late");
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":second_request["id"],"result":{"ok":true}})
            .to_string()
            .into(),
    ))
    .await
    .expect("current");
    assert_eq!(
        second.await.expect("join").expect("second"),
        json!({"ok":true})
    );
}

#[tokio::test]
async fn capacity_is_fail_fast_and_aborting_future_releases_permit() {
    let (client, mut peer) = registered(1).await;
    let first = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Value>("hooks.upstream.decode", &json!({}))
                .await
        }
    });
    let first_request = next_request(&mut peer).await;
    assert!(matches!(
        client
            .call::<_, Value>("hooks.upstream.decode", &json!({}))
            .await,
        Err(ExternalPackageConnectionError::Busy)
    ));
    first.abort();
    let _ = first.await;
    tokio::task::yield_now().await;
    let second = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .call::<_, Value>("hooks.upstream.decode", &json!({}))
                .await
        }
    });
    let second_request = next_request(&mut peer).await;
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":first_request["id"],"result":{"late":true}})
            .to_string()
            .into(),
    ))
    .await
    .expect("late");
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":second_request["id"],"result":{"ok":true}})
            .to_string()
            .into(),
    ))
    .await
    .expect("current");
    assert_eq!(
        second.await.expect("join").expect("second"),
        json!({"ok":true})
    );
}

#[tokio::test(start_paused = true)]
async fn heartbeat_remains_active_while_rpc_is_slow() {
    let config = ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(20),
        Duration::from_secs(10),
        Duration::from_secs(30),
        1,
        1024 * 1024,
        1024 * 1024,
        1024 * 1024,
        128 * 1024,
    );
    let (client, mut peer) = registered_with_config(config).await;
    let call = tokio::spawn(async move {
        client
            .call::<_, Value>("hooks.upstream.decode", &json!({}))
            .await
    });
    let request = next_request(&mut peer).await;
    tokio::time::advance(Duration::from_secs(10)).await;
    let Message::Ping(payload) = peer.next().await.expect("ping").expect("valid ping") else {
        panic!("expected ping")
    };
    peer.send(Message::Pong(payload)).await.expect("pong");
    peer.send(Message::Text(
        json!({"jsonrpc":"2.0","id":request["id"],"result":{"ok":true}})
            .to_string()
            .into(),
    ))
    .await
    .expect("response");
    assert_eq!(call.await.expect("join").expect("call"), json!({"ok":true}));
}

#[tokio::test(start_paused = true)]
async fn registration_times_out_at_thirty_seconds_without_sleep_races() {
    let (actor_socket, _peer) = raw_pair().await;
    let connecting = tokio::spawn(ExternalPackageClient::connect(
        actor_socket,
        9,
        ExternalPackageConnectionConfig::default(),
    ));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(
        matches!(connecting.await.expect("join").expect_err("timeout"), ExternalPackageConnectionError::Timeout { ref method, .. } if method == "package.register")
    );
}

#[tokio::test]
async fn cancelling_connection_attempt_aborts_the_registration_actor() {
    let (actor_socket, mut peer) = raw_pair().await;
    let connecting = tokio::spawn(ExternalPackageClient::connect(
        actor_socket,
        10,
        ExternalPackageConnectionConfig::default(),
    ));
    let _ = next_request(&mut peer).await;
    connecting.abort();
    let _ = connecting.await;
    let closed = tokio::time::timeout(Duration::from_secs(1), peer.next())
        .await
        .expect("registration transport closes promptly");
    assert!(closed.is_none() || matches!(closed, Some(Err(_))));
}

#[tokio::test]
async fn disconnect_is_idempotent_across_clones_and_emits_close_frame() {
    let (client, mut peer) = registered(1).await;
    let first = tokio::spawn({
        let client = client.clone();
        async move { client.disconnect().await }
    });
    assert!(matches!(
        peer.next()
            .await
            .expect("close frame")
            .expect("valid close"),
        Message::Close(_)
    ));
    first.await.expect("first disconnect joins");
    client.disconnect().await;
}
