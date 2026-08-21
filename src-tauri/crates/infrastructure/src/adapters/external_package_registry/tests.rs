use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use intercept_proxy_application::{
    ExternalPackageApplicationPort, ExternalPackageServiceStateViewModel,
};
use intercept_proxy_domain::ExternalPackageRegistration;
use serde_json::Value;
use tokio::io::DuplexStream;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

use super::*;

type Peer = WebSocketStream<DuplexStream>;

fn registration(name: &str) -> ExternalPackageRegistration {
    serde_json::from_value(serde_json::json!({
        "api": 1,
        "package": {
            "id": "vendor-iso8583", "name": name, "version": "1.0.0",
            "description": "external test package"
        },
        "document": {
            "upstream": {
                "schema": {
                    "id": "vendor-upstream", "version": 1, "title": "Upstream",
                    "fields": [
                        {"name": "mti", "label": "MTI", "type": "string"},
                        {"name": "amount", "label": "Amount", "type": "int"},
                        {"name": "approved", "label": "Approved", "type": "bool"},
                        {"name": "raw", "label": "Raw", "type": "blob"}
                    ]
                },
                "display": "render"
            },
            "downstream": {
                "schema": {
                    "id": "vendor-downstream", "version": 1, "title": "Downstream",
                    "fields": [{"name": "code", "label": "Code", "type": "string"}]
                },
                "display": "render"
            }
        },
        "hooks": {
            "upstream": {"frame": "frame", "decode": "decode", "encode": "encode"},
            "downstream": {"frame": "frame", "decode": "decode", "encode": "encode"}
        }
    }))
    .expect("valid external registration")
}

mod coverage;
mod diagnostics;

async fn connected_client(
    registration: &ExternalPackageRegistration,
    generation: u64,
) -> (ExternalPackageClient, Peer) {
    let (actor_io, peer_io) = tokio::io::duplex(2 * 1024 * 1024);
    let actor = WebSocketStream::from_raw_socket(actor_io, Role::Server, None).await;
    let mut peer = WebSocketStream::from_raw_socket(peer_io, Role::Client, None).await;
    let config = super::super::external_packages::ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(10),
        Duration::from_secs(30),
        4,
        1024 * 1024,
        1024 * 1024,
        1024 * 1024,
        128 * 1024,
    );
    let connecting = tokio::spawn(ExternalPackageClient::connect(actor, generation, config));
    let Message::Text(request) = peer
        .next()
        .await
        .expect("registration request")
        .expect("valid WebSocket frame")
    else {
        panic!("registration request must be text")
    };
    let request: Value = serde_json::from_str(&request).expect("valid JSON-RPC request");
    peer.send(Message::Text(
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": registration,
        })
        .to_string()
        .into(),
    ))
    .await
    .expect("registration response");
    let (returned_registration, client) = connecting
        .await
        .expect("actor task")
        .expect("registered client");
    assert_eq!(&returned_registration, registration);
    (client, peer)
}

#[test]
fn fingerprint_is_stable_and_covers_metadata() {
    let first = registration("First");
    let same: ExternalPackageRegistration =
        serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    let changed = registration("Changed");

    assert_eq!(
        external_package_registration_fingerprint(&first).unwrap(),
        external_package_registration_fingerprint(&same).unwrap()
    );
    assert_ne!(
        external_package_registration_fingerprint(&first).unwrap(),
        external_package_registration_fingerprint(&changed).unwrap()
    );
}

#[tokio::test]
async fn service_status_is_explicit_and_never_claims_authentication() {
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    let initial = registry.service_status().await.unwrap();
    assert!(matches!(
        initial.state,
        ExternalPackageServiceStateViewModel::Failed { .. }
    ));
    assert_eq!(initial.fixed_path, "/packages");
    assert!(!initial.authentication_enabled);

    registry.mark_service_listening("ws://127.0.0.1:9000/packages");
    let listening = registry.service_status().await.unwrap();
    assert_eq!(listening.websocket_url, "ws://127.0.0.1:9000/packages");
    assert_eq!(listening.online_connection_count, 0);
    assert_eq!(
        listening.state,
        ExternalPackageServiceStateViewModel::Listening
    );

    registry.mark_service_failed("ws://127.0.0.1:9000/packages", "端口已被其他进程占用。");
    assert!(matches!(
        registry.service_status().await.unwrap().state,
        ExternalPackageServiceStateViewModel::Failed { .. }
    ));
}

#[test]
fn runtime_provider_distinguishes_internal_disabled_and_offline() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = ExternalPackageRegistryAdapter::new(Arc::clone(&store));
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let unknown = intercept_proxy_domain::ProtocolPackageRef {
        id: intercept_proxy_domain::ProtocolPackageId::new("unknown-package").unwrap(),
        version: intercept_proxy_domain::ProtocolPackageVersion::new("1.0.0").unwrap(),
    };

    assert!(
        ExternalSocketPackageProvider::resolve(&registry, &unknown)
            .unwrap()
            .is_none()
    );
    store
        .accept_external_package_registration(
            &registration,
            external_package_registration_fingerprint(&registration).unwrap(),
            Utc::now(),
        )
        .unwrap();
    let disabled = ExternalSocketPackageProvider::resolve(&registry, &package).unwrap_err();
    assert_eq!(disabled.view_model.code, "EXTERNAL_PACKAGE_DISABLED");

    assert!(store.set_external_package_enabled(&package, true).unwrap());
    let offline = ExternalSocketPackageProvider::resolve(&registry, &package).unwrap_err();
    assert_eq!(offline.view_model.code, "EXTERNAL_PACKAGE_OFFLINE");
}

#[tokio::test]
async fn persisted_records_restart_offline_and_keep_enabled_state() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    store
        .accept_external_package_registration(
            &registration,
            external_package_registration_fingerprint(&registration).unwrap(),
            Utc::now(),
        )
        .unwrap();
    assert!(store.set_external_package_enabled(&package, true).unwrap());

    let restarted = ExternalPackageRegistryAdapter::new(store);
    let version = restarted
        .get(&package)
        .await
        .unwrap()
        .expect("persisted package");
    assert!(version.enabled);
    assert_eq!(version.source.external_online(), Some(false));
    assert_eq!(restarted.describe(&package).await.unwrap().package, package);
}

#[tokio::test]
async fn duplicate_online_is_rejected_and_reconnect_keeps_user_enablement() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = ExternalPackageRegistryAdapter::new(store);
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (first_client, _first_peer) = connected_client(&registration, 1).await;
    let invalid_fingerprint = registry
        .accept_registration(&registration, [0_u8; 32], first_client.clone())
        .unwrap_err();
    assert_eq!(
        invalid_fingerprint.view_model.code,
        "EXTERNAL_PACKAGE_FINGERPRINT_INVALID"
    );
    assert!(registry.get(&package).await.unwrap().is_none());
    let first = registry
        .accept_registration(&registration, fingerprint, first_client.clone())
        .unwrap();
    assert!(!first.enabled);
    registry.set_enabled(&package, true).await.unwrap();

    let (duplicate_client, _duplicate_peer) = connected_client(&registration, 2).await;
    let duplicate = registry
        .accept_registration(&registration, fingerprint, duplicate_client.clone())
        .unwrap_err();
    assert_eq!(duplicate.view_model.code, "EXTERNAL_PACKAGE_ALREADY_ONLINE");
    duplicate_client.disconnect().await;

    registry.disconnect(&package).await.unwrap();
    assert_eq!(
        registry
            .get(&package)
            .await
            .unwrap()
            .unwrap()
            .source
            .external_online(),
        Some(false)
    );
    let (reconnected_client, _reconnected_peer) = connected_client(&registration, 3).await;
    let reconnected = registry
        .accept_registration(&registration, fingerprint, reconnected_client)
        .unwrap();
    assert!(reconnected.enabled);
    assert!(!registry.mark_disconnected(&package, first.connection_id));
    assert!(
        registry
            .record_remote_address(
                &package,
                reconnected.connection_id,
                "127.0.0.1:49153".parse().unwrap(),
            )
            .unwrap()
    );
    assert!(
        !registry
            .record_connection_error(
                &package,
                first.connection_id,
                &ExternalPackageConnectionError::Transport("stale secret".to_owned()),
            )
            .unwrap()
    );
    let detail = registry.detail(&package).await.unwrap();
    assert_eq!(detail.remote_address.as_deref(), Some("127.0.0.1:49153"));
    assert_eq!(detail.recent_error, None);
    assert_eq!(
        registry
            .service_status()
            .await
            .unwrap()
            .online_connection_count,
        1
    );
    registry.disconnect(&package).await.unwrap();
    // API 边界对重复断连保持幂等，且不会改变持久化启用位。
    registry.disconnect(&package).await.unwrap();
    assert!(registry.get(&package).await.unwrap().unwrap().enabled);
}

#[tokio::test]
async fn detail_projects_connection_fingerprint_methods_timeout_and_recent_error() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = ExternalPackageRegistryAdapter::new(store);
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 30).await;
    let accepted = registry
        .accept_registration(&registration, fingerprint, client)
        .unwrap();
    registry
        .record_remote_address(
            &package,
            accepted.connection_id,
            "127.0.0.1:49152".parse().unwrap(),
        )
        .unwrap();
    registry
        .record_connection_error(
            &package,
            accepted.connection_id,
            &ExternalPackageConnectionError::Disconnected,
        )
        .unwrap();

    let detail = registry.detail(&package).await.unwrap();
    assert_eq!(detail.remote_address.as_deref(), Some("127.0.0.1:49152"));
    assert_eq!(detail.connection_id, Some(accepted.connection_id.as_uuid()));
    assert_eq!(detail.registration_fingerprint_sha256.len(), 64);
    assert_eq!(detail.rpc_timeout_seconds, 5);
    assert_eq!(detail.upstream_methods.frame, "hooks.upstream.frame");
    assert_eq!(
        detail.downstream_methods.display,
        "document.downstream.render"
    );
    assert_eq!(
        detail.recent_error.expect("recent error").code,
        "EXTERNAL_PACKAGE_DISCONNECTED"
    );
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test]
async fn detail_restores_safe_connection_history_without_faking_online_state() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("external-package-history.sqlite3");
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let remote_address = "127.0.0.1:49152".parse().unwrap();

    {
        let store = Arc::new(SqliteStore::open(&database).unwrap());
        let registry = ExternalPackageRegistryAdapter::new(store);
        let (client, _peer) = connected_client(&registration, 31).await;
        let accepted = registry
            .accept_registration(&registration, fingerprint, client)
            .unwrap();
        registry
            .record_remote_address(&package, accepted.connection_id, remote_address)
            .unwrap();
        registry
            .record_connection_error(
                &package,
                accepted.connection_id,
                &ExternalPackageConnectionError::Disconnected,
            )
            .unwrap();
        assert!(registry.mark_disconnected(&package, accepted.connection_id));
    }

    let restarted =
        ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::open(&database).unwrap()));
    let summary = restarted.get(&package).await.unwrap().expect("summary");
    assert_eq!(summary.source.external_online(), Some(false));
    let detail = restarted.detail(&package).await.unwrap();
    assert_eq!(detail.remote_address.as_deref(), Some("127.0.0.1:49152"));
    assert_eq!(detail.connection_id, None);
    assert_eq!(
        detail.recent_error.expect("persisted recent error").code,
        "EXTERNAL_PACKAGE_DISCONNECTED"
    );
}

#[tokio::test]
async fn online_delete_closes_connection_and_next_registration_is_first_install() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = ExternalPackageRegistryAdapter::new(store);
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 4).await;
    registry
        .accept_registration(&registration, fingerprint, client)
        .unwrap();
    registry.set_enabled(&package, true).await.unwrap();

    registry.delete(&package).await.unwrap();
    registry.delete(&package).await.unwrap();
    assert!(registry.get(&package).await.unwrap().is_none());

    let (new_client, _new_peer) = connected_client(&registration, 5).await;
    let new_registration = registry
        .accept_registration(&registration, fingerprint, new_client)
        .unwrap();
    assert!(!new_registration.enabled);
    registry.disconnect(&package).await.unwrap();
}
