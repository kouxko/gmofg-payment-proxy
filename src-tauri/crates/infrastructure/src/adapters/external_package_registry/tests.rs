use std::time::Duration;

use futures_util::SinkExt;
use intercept_proxy_application::{
    ExternalPackageApplicationPort, ExternalPackageServiceStateViewModel,
};
use intercept_proxy_package_contract::{PackageManifest, PackageRegisterNotification};
use serde_json::Value;
use tokio::io::DuplexStream;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

use super::*;

type Peer = WebSocketStream<DuplexStream>;

fn registration(name: &str) -> PackageManifest {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/socket-manifest.json"
    ))
    .expect("valid package fixture");
    value["package"]["id"] = Value::String("vendor-iso8583".to_owned());
    value["package"]["name"] = Value::String(name.to_owned());
    serde_json::from_value(value).expect("valid external registration")
}

fn registration_version(version: &str) -> PackageManifest {
    let mut value: Value = serde_json::to_value(registration(version)).unwrap();
    value["package"]["version"] = Value::String(version.to_owned());
    serde_json::from_value(value).unwrap()
}

mod async_persistence;
mod coverage;
mod diagnostics;
mod environment_apply_gate_revision16;
mod error_views;
mod lifecycle;

#[tokio::test]
async fn application_list_orders_versions_by_semver_not_sql_text() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    for version in ["10.0.0", "2.0.0"] {
        let registration = registration_version(version);
        let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
        store
            .accept_external_package_registration(&registration, fingerprint, chrono::Utc::now())
            .unwrap();
    }
    let registry = ExternalPackageRegistryAdapter::new(store);
    let versions = registry.list().await.unwrap();
    assert_eq!(versions[0].package.version.as_str(), "2.0.0");
    assert_eq!(versions[1].package.version.as_str(), "10.0.0");
}

async fn connected_client(
    registration: &PackageManifest,
    generation: u64,
) -> (PackageTransportClient, Peer) {
    let (actor_io, peer_io) = tokio::io::duplex(2 * 1024 * 1024);
    let actor = WebSocketStream::from_raw_socket(actor_io, Role::Server, None).await;
    let mut peer = WebSocketStream::from_raw_socket(peer_io, Role::Client, None).await;
    let config = crate::adapters::PackageTransportConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(10),
        Duration::from_secs(30),
        1024 * 1024,
        1024 * 1024,
        1024 * 1024,
        128 * 1024,
    );
    let connecting = tokio::spawn(PackageTransportClient::connect(actor, generation, config));
    peer.send(Message::Text(
        serde_json::to_string(&PackageRegisterNotification::new(registration.clone()))
            .expect("registration notification")
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
    let same: PackageManifest =
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

    registry
        .mark_service_listening("ws://127.0.0.1:9000/packages")
        .await;
    let listening = registry.service_status().await.unwrap();
    assert_eq!(listening.websocket_url, "ws://127.0.0.1:9000/packages");
    assert_eq!(listening.online_connection_count, 0);
    assert_eq!(
        listening.state,
        ExternalPackageServiceStateViewModel::Listening
    );

    registry
        .mark_service_failed("ws://127.0.0.1:9000/packages", "端口已被其他进程占用。")
        .await;
    assert!(matches!(
        registry.service_status().await.unwrap().state,
        ExternalPackageServiceStateViewModel::Failed { .. }
    ));
}

#[tokio::test]
async fn runtime_provider_distinguishes_internal_disabled_and_offline() {
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
            .await
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
    let offline = ExternalSocketPackageProvider::resolve(&registry, &package)
        .await
        .unwrap_err();
    assert_eq!(offline.view_model.code, "EXTERNAL_PACKAGE_OFFLINE");

    assert!(store.set_external_package_enabled(&package, false).unwrap());
    let disabled = ExternalSocketPackageProvider::resolve(&registry, &package)
        .await
        .unwrap_err();
    assert_eq!(disabled.view_model.code, "EXTERNAL_PACKAGE_DISABLED");
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
async fn remote_registration_cannot_claim_an_identity_with_a_local_component() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = ExternalPackageRegistryAdapter::new(Arc::clone(&store));
    let registration = registration("Locally managed ISO8583");
    store
        .install_local_external_package(&registration, b"component-bytes", Utc::now())
        .unwrap();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 1).await;

    let error = registry
        .accept_registration(&registration, fingerprint, client)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_SOURCE_CONFLICT");
    assert!(
        registry
            .client(&registration.package().identity())
            .is_none()
    );
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
        .await
        .unwrap_err();
    assert_eq!(
        invalid_fingerprint.view_model.code,
        "EXTERNAL_PACKAGE_FINGERPRINT_INVALID"
    );
    assert!(registry.get(&package).await.unwrap().is_none());
    let first = registry
        .accept_registration(&registration, fingerprint, first_client.clone())
        .await
        .unwrap();
    assert!(first.enabled);
    registry.set_enabled(&package, false).await.unwrap();

    let (duplicate_client, _duplicate_peer) = connected_client(&registration, 2).await;
    let duplicate = registry
        .accept_registration(&registration, fingerprint, duplicate_client.clone())
        .await
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
        .await
        .unwrap();
    assert!(!reconnected.enabled);
    assert!(
        !registry
            .mark_disconnected(&package, first.connection_id)
            .await
    );
    assert!(
        registry
            .record_remote_address(
                &package,
                reconnected.connection_id,
                "127.0.0.1:49153".parse().unwrap(),
            )
            .await
            .unwrap()
    );
    assert!(
        !registry
            .record_connection_error(
                &package,
                first.connection_id,
                &PackageTransportError::Transport("stale secret".to_owned()),
            )
            .await
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
    // API 边界对重复断连保持幂等，且保留用户显式停用位。
    registry.disconnect(&package).await.unwrap();
    assert!(!registry.get(&package).await.unwrap().unwrap().enabled);
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
        .await
        .unwrap();
    registry
        .record_remote_address(
            &package,
            accepted.connection_id,
            "127.0.0.1:49152".parse().unwrap(),
        )
        .await
        .unwrap();
    registry
        .record_connection_error(
            &package,
            accepted.connection_id,
            &PackageTransportError::Disconnected,
        )
        .await
        .unwrap();

    let detail = registry.detail(&package).await.unwrap();
    assert_eq!(detail.remote_address.as_deref(), Some("127.0.0.1:49152"));
    assert_eq!(detail.connection_id, Some(accepted.connection_id.as_uuid()));
    assert_eq!(detail.registration_fingerprint_sha256.len(), 64);
    assert_eq!(detail.upstream_methods.frame, "hooks.upstream.frame");
    assert_eq!(
        detail.downstream_methods.display,
        "document.downstream.display"
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
            .await
            .unwrap();
        registry
            .record_remote_address(&package, accepted.connection_id, remote_address)
            .await
            .unwrap();
        registry
            .record_connection_error(
                &package,
                accepted.connection_id,
                &PackageTransportError::Disconnected,
            )
            .await
            .unwrap();
        assert!(
            registry
                .mark_disconnected(&package, accepted.connection_id)
                .await
        );
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
        .await
        .unwrap();
    registry.set_enabled(&package, true).await.unwrap();

    registry.delete(&package).await.unwrap();
    registry.delete(&package).await.unwrap();
    assert!(registry.get(&package).await.unwrap().is_none());

    let (new_client, _new_peer) = connected_client(&registration, 5).await;
    let new_registration = registry
        .accept_registration(&registration, fingerprint, new_client)
        .await
        .unwrap();
    assert!(new_registration.enabled);
    registry.disconnect(&package).await.unwrap();
}
