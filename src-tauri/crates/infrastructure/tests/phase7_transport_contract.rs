use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use intercept_proxy_domain::ErrorCode;
use intercept_proxy_infrastructure::{
    PackageTransportClient, PackageTransportConfig, PackageTransportError,
};
use intercept_proxy_package_contract::{
    CanonicalBase64, DecodeParams, FrameParams, PackageManifest, PackageRegisterNotification,
};
use tokio::io::duplex;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

const MANIFEST: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/socket-manifest.json"
);

async fn connected() -> (
    PackageTransportClient,
    WebSocketStream<tokio::io::DuplexStream>,
) {
    let (proxy_io, package_io) = duplex(64 * 1024);
    let proxy = WebSocketStream::from_raw_socket(proxy_io, Role::Server, None).await;
    let mut package = WebSocketStream::from_raw_socket(package_io, Role::Client, None).await;
    let connecting = tokio::spawn(PackageTransportClient::connect(
        proxy,
        7,
        PackageTransportConfig::new(
            Duration::from_secs(1),
            Duration::from_mins(1),
            Duration::from_mins(2),
            8 * 1024 * 1024,
            8 * 1024 * 1024,
            1024 * 1024,
            128 * 1024,
        ),
    ));
    let manifest: PackageManifest = serde_json::from_str(MANIFEST).expect("manifest");
    package
        .send(Message::Text(
            serde_json::to_string(&PackageRegisterNotification::new(manifest))
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
    let (registered, client) = connecting.await.unwrap().unwrap();
    assert_eq!(
        registered.kind(),
        intercept_proxy_package_contract::PackageKind::Socket
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), package.next())
            .await
            .is_err(),
        "registration notification has no reply"
    );
    (client, package)
}

#[tokio::test]
async fn package_initiates_idless_registration_and_proxy_sends_no_reply() {
    let (_client, _package) = connected().await;
}

#[tokio::test]
async fn fixed_decode_request_and_typed_result_use_shared_contract() {
    let (client, mut package) = connected().await;
    let task = tokio::spawn(async move {
        client
            .upstream_decode(DecodeParams {
                input: "AA==".into(),
            })
            .await
    });
    let request: serde_json::Value =
        serde_json::from_str(&package.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    assert_eq!(request["method"], "hooks.upstream.decode");
    let id = request["id"].as_str().unwrap();
    package
        .send(Message::Text(
            format!(r#"{{"jsonrpc":"2.0","id":"{id}","result":{{"ok":true}}}}"#).into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(task.await.unwrap().unwrap()).unwrap(),
        serde_json::json!({"ok": true})
    );
}

#[tokio::test]
async fn null_document_is_a_present_success_result() {
    let (client, mut package) = connected().await;
    let task = tokio::spawn(async move {
        client
            .upstream_decode(DecodeParams {
                input: "AA==".into(),
            })
            .await
    });
    let request: serde_json::Value =
        serde_json::from_str(&package.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    let id = request["id"].as_str().unwrap();
    package
        .send(Message::Text(
            format!(r#"{{"jsonrpc":"2.0","id":"{id}","result":null}}"#).into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(task.await.unwrap().unwrap()).unwrap(),
        serde_json::Value::Null
    );
}

#[tokio::test]
async fn frame_result_is_validated_against_the_sent_buffer() {
    let (client, mut package) = connected().await;
    let task = tokio::spawn(async move {
        client
            .upstream_frame(FrameParams {
                buffer: CanonicalBase64::from_bytes(b"abc"),
            })
            .await
    });
    let request: serde_json::Value =
        serde_json::from_str(&package.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    let id = request["id"].as_str().unwrap();
    package.send(Message::Text(format!(r#"{{"jsonrpc":"2.0","id":"{id}","result":{{"status":"complete","consumedBytes":4}}}}"#).into())).await.unwrap();
    let error = task
        .await
        .unwrap()
        .expect_err("oversized consumedBytes must fail");
    assert!(
        matches!(error, PackageTransportError::Package { ref error } if error.code == ErrorCode::ProtocolPackageInvalid)
    );
}

#[tokio::test]
async fn cancelling_pre_registration_connect_drops_the_silent_peer() {
    let (proxy_io, package_io) = duplex(1024);
    let proxy = WebSocketStream::from_raw_socket(proxy_io, Role::Server, None).await;
    let mut package = WebSocketStream::from_raw_socket(package_io, Role::Client, None).await;
    let task = tokio::spawn(PackageTransportClient::connect(
        proxy,
        1,
        PackageTransportConfig::new(
            Duration::from_secs(30),
            Duration::from_secs(10),
            Duration::from_secs(30),
            8,
            1024,
            1024,
            1024,
        ),
    ));
    task.abort();
    let _ = task.await;
    assert!(
        tokio::time::timeout(Duration::from_secs(1), package.next())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn raw_logical_frame_limit_is_independent_from_encoded_wire_budget() {
    let (proxy_io, package_io) = duplex(4096);
    let proxy = WebSocketStream::from_raw_socket(proxy_io, Role::Server, None).await;
    let mut package = WebSocketStream::from_raw_socket(package_io, Role::Client, None).await;
    let connecting = tokio::spawn(PackageTransportClient::connect(
        proxy,
        1,
        PackageTransportConfig::new(
            Duration::from_secs(1),
            Duration::from_mins(1),
            Duration::from_mins(2),
            3,
            1024,
            1024,
            1024,
        ),
    ));
    let manifest: PackageManifest = serde_json::from_str(MANIFEST).unwrap();
    package
        .send(Message::Text(
            serde_json::to_string(&PackageRegisterNotification::new(manifest))
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
    let (_, client) = connecting.await.unwrap().unwrap();
    let at_limit = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .upstream_frame(FrameParams {
                    buffer: CanonicalBase64::from_bytes(b"abc"),
                })
                .await
        }
    });
    let request: serde_json::Value =
        serde_json::from_str(&package.next().await.unwrap().unwrap().into_text().unwrap()).unwrap();
    let id = request["id"].as_str().unwrap();
    package
        .send(Message::Text(
            format!(r#"{{"jsonrpc":"2.0","id":"{id}","result":{{"status":"need_more"}}}}"#).into(),
        ))
        .await
        .unwrap();
    at_limit.await.unwrap().unwrap();
    assert!(matches!(
        client
            .upstream_frame(FrameParams {
                buffer: CanonicalBase64::from_bytes(b"abcd")
            })
            .await,
        Err(PackageTransportError::MessageTooLarge {
            actual_bytes: 4,
            limit_bytes: 3
        })
    ));
}

#[tokio::test]
async fn many_sequential_rpc_ids_do_not_accumulate_and_duplicate_reply_fails_closed() {
    let (mut client, mut package) = connected().await;
    let mut last_id = String::new();
    for sequence in 0..256 {
        let call = tokio::spawn({
            let client = client.clone();
            async move {
                client
                    .upstream_decode(DecodeParams {
                        input: "AA==".into(),
                    })
                    .await
            }
        });
        let request: serde_json::Value =
            serde_json::from_str(&package.next().await.unwrap().unwrap().into_text().unwrap())
                .unwrap();
        last_id = request["id"].as_str().unwrap().to_owned();
        package
            .send(Message::Text(
                format!(
                    r#"{{"jsonrpc":"2.0","id":"{last_id}","result":{{"sequence":{sequence}}}}}"#
                )
                .into(),
            ))
            .await
            .unwrap();
        call.await.unwrap().unwrap();
    }
    package
        .send(Message::Text(
            format!(r#"{{"jsonrpc":"2.0","id":"{last_id}","result":null}}"#).into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        client.wait_closed().await,
        PackageTransportError::InvalidResponse
    ));
}
