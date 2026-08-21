use intercept_proxy_application::UiEventPayload;

use super::*;
use crate::adapters::external_packages::{
    ExternalPackageFatalProtocolError, ExternalPackageRemoteError,
};

#[tokio::test]
async fn list_projects_each_persisted_package_with_current_online_state() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = ExternalPackageRegistryAdapter::new(Arc::clone(&store));
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 40).await;
    registry
        .accept_registration(&registration, fingerprint, client)
        .unwrap();

    let listed = registry.list().await.unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].package, package);
    assert_eq!(listed[0].source.external_online(), Some(true));
    registry.disconnect(&package).await.unwrap();
}

fn missing_registry() -> (ExternalPackageRegistryAdapter, ProtocolPackageRef) {
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    let package = registration("Missing").package().identity().clone();
    (registry, package)
}

fn registry_without_external_table() -> (ExternalPackageRegistryAdapter, ProtocolPackageRef) {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store.remove_external_package_table_for_test();
    let package = registration("Missing").package().identity().clone();
    (ExternalPackageRegistryAdapter::new(store), package)
}

#[tokio::test]
async fn describe_missing_package_returns_not_found() {
    let (registry, package) = missing_registry();

    let error = registry.describe(&package).await.unwrap_err();

    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_NOT_FOUND");
    assert_eq!(
        error.view_model.entity_id.as_deref(),
        Some("vendor-iso8583@1.0.0")
    );
}

#[tokio::test]
async fn detail_missing_package_returns_not_found() {
    let (registry, package) = missing_registry();

    let error = registry.detail(&package).await.unwrap_err();

    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_NOT_FOUND");
}

#[tokio::test]
async fn enabling_missing_package_returns_not_found() {
    let (registry, package) = missing_registry();

    let error = registry.set_enabled(&package, true).await.unwrap_err();

    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_NOT_FOUND");
}

#[tokio::test]
async fn list_projects_database_failure_as_application_error() {
    let (registry, _package) = registry_without_external_table();

    let error = registry.list().await.unwrap_err();

    assert_eq!(error.view_model.code, "INTERNAL_ERROR");
}

#[tokio::test]
async fn get_projects_database_failure_as_application_error() {
    let (registry, package) = registry_without_external_table();

    let error = registry.get(&package).await.unwrap_err();

    assert_eq!(error.view_model.code, "INTERNAL_ERROR");
}

#[tokio::test]
async fn describe_projects_database_failure_as_application_error() {
    let (registry, package) = registry_without_external_table();

    let error = registry.describe(&package).await.unwrap_err();

    assert_eq!(error.view_model.code, "INTERNAL_ERROR");
}

#[tokio::test]
async fn detail_projects_database_failure_as_application_error() {
    let (registry, package) = registry_without_external_table();

    let error = registry.detail(&package).await.unwrap_err();

    assert_eq!(error.view_model.code, "INTERNAL_ERROR");
}

#[tokio::test]
async fn changed_registration_for_persisted_identity_is_rejected_without_becoming_online() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let original = registration("Original");
    let changed = registration("Changed");
    let package = original.package().identity().clone();
    store
        .accept_external_package_registration(
            &original,
            external_package_registration_fingerprint(&original).unwrap(),
            Utc::now(),
        )
        .unwrap();
    let registry = ExternalPackageRegistryAdapter::new(store);
    let (client, _peer) = connected_client(&changed, 41).await;

    let error = registry
        .accept_registration(
            &changed,
            external_package_registration_fingerprint(&changed).unwrap(),
            client,
        )
        .unwrap_err();

    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_IDENTITY_CONFLICT");
    assert!(registry.client(&package).is_none());
}

#[tokio::test]
async fn database_failure_during_registration_never_publishes_online_client() {
    let (registry, package) = registry_without_external_table();
    let registration = registration("Vendor ISO8583");
    let (client, _peer) = connected_client(&registration, 45).await;

    let error = registry
        .accept_registration(
            &registration,
            external_package_registration_fingerprint(&registration).unwrap(),
            client,
        )
        .unwrap_err();

    assert_eq!(error.view_model.code, "INTERNAL_ERROR");
    assert!(registry.client(&package).is_none());
}

#[tokio::test]
async fn enabled_online_package_resolves_to_runtime_binding() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = ExternalPackageRegistryAdapter::new(store);
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 42).await;
    registry
        .accept_registration(&registration, fingerprint, client)
        .unwrap();
    registry.set_enabled(&package, true).await.unwrap();

    let binding = ExternalSocketPackageProvider::resolve(&registry, &package)
        .unwrap()
        .expect("external binding");

    assert_eq!(binding.registration().package().identity(), &package);
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test]
async fn configured_event_hub_receives_service_and_catalog_changes() {
    let events = Arc::new(EventHub::new(16));
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    registry.set_event_hub(Arc::clone(&events));
    registry.mark_service_listening("ws://127.0.0.1:9000/packages");
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let (client, _peer) = connected_client(&registration, 43).await;
    registry
        .accept_registration(
            &registration,
            external_package_registration_fingerprint(&registration).unwrap(),
            client,
        )
        .unwrap();

    let replay = events.replay_after(0);

    assert!(replay.events.iter().any(|event| matches!(
        event.payload,
        UiEventPayload::ExternalPackageServiceStatusChanged(_)
    )));
    assert!(replay.events.iter().any(|event| matches!(
        &event.payload,
        UiEventPayload::ProtocolPackageCatalogChanged { package: changed } if changed == &package
    )));
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test]
async fn external_package_lifecycle_is_queryable_as_redacted_diagnostics() {
    let events = Arc::new(EventHub::new(32));
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    registry.set_event_hub(Arc::clone(&events));
    registry.mark_service_listening("ws://127.0.0.1:9000/packages");
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let (client, _peer) = connected_client(&registration, 51).await;
    let accepted = registry
        .accept_registration(
            &registration,
            external_package_registration_fingerprint(&registration).unwrap(),
            client,
        )
        .unwrap();
    registry
        .record_remote_address(
            &package,
            accepted.connection_id,
            "127.0.0.1:49051".parse().unwrap(),
        )
        .unwrap();
    registry
        .record_connection_error(
            &package,
            accepted.connection_id,
            &ExternalPackageConnectionError::Transport(
                "password=do-not-leak; peer reset".to_owned(),
            ),
        )
        .unwrap();

    let rows = events.diagnostic_log_snapshot();
    let rendered = rows
        .iter()
        .map(|row| {
            format!(
                "{} {}",
                row.summary,
                row.detail.as_deref().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("外部软件包服务正在监听"));
    assert!(rendered.contains("vendor-iso8583@1.0.0"));
    assert!(rendered.contains(&accepted.connection_id.as_uuid().to_string()));
    assert!(rendered.contains("127.0.0.1:49051"));
    assert!(rendered.contains("EXTERNAL_PACKAGE_TRANSPORT_ERROR"));
    assert!(!rendered.contains("do-not-leak"));
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test]
async fn stale_generation_updates_do_not_mutate_connection_history() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = ExternalPackageRegistryAdapter::new(store);
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let (client, _peer) = connected_client(&registration, 44).await;
    let accepted = registry
        .accept_registration(
            &registration,
            external_package_registration_fingerprint(&registration).unwrap(),
            client,
        )
        .unwrap();
    let stale = ExternalPackageConnectionId(Uuid::new_v4());

    assert!(
        !registry
            .record_remote_address(&package, stale, "127.0.0.1:49000".parse().unwrap())
            .unwrap()
    );
    assert!(
        !registry
            .record_connection_error(
                &package,
                stale,
                &ExternalPackageConnectionError::Disconnected,
            )
            .unwrap()
    );
    assert!(!registry.mark_disconnected(&package, stale));
    assert!(registry.client(&package).is_some());
    registry.disconnect(&package).await.unwrap();
    assert!(!registry.mark_disconnected(&package, accepted.connection_id));
}

#[tokio::test]
async fn old_generation_address_cannot_cross_new_online_publication_window() {
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (first_client, _first_peer) = connected_client(&registration, 46).await;
    let first = registry
        .accept_registration(&registration, fingerprint, first_client)
        .unwrap();
    let (next_client, _next_peer) = connected_client(&registration, 47).await;
    let next_id = ExternalPackageConnectionId(Uuid::new_v4());
    registry.online.lock().insert(
        package.clone(),
        OnlineConnection::Active {
            id: next_id,
            client: next_client,
        },
    );

    let recorded = registry
        .record_remote_address(
            &package,
            first.connection_id,
            "127.0.0.1:49002".parse().unwrap(),
        )
        .unwrap();

    assert!(!recorded);
    assert_eq!(
        registry
            .store
            .get_external_package(&package)
            .unwrap()
            .unwrap()
            .remote_address,
        None
    );
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test]
async fn old_generation_error_cannot_cross_new_online_publication_window() {
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (first_client, _first_peer) = connected_client(&registration, 48).await;
    let first = registry
        .accept_registration(&registration, fingerprint, first_client)
        .unwrap();
    let (next_client, _next_peer) = connected_client(&registration, 49).await;
    let next_id = ExternalPackageConnectionId(Uuid::new_v4());
    registry.online.lock().insert(
        package.clone(),
        OnlineConnection::Active {
            id: next_id,
            client: next_client,
        },
    );

    let recorded = registry
        .record_connection_error(
            &package,
            first.connection_id,
            &ExternalPackageConnectionError::Transport("old generation".to_owned()),
        )
        .unwrap();

    assert!(!recorded);
    assert_eq!(
        registry
            .store
            .get_external_package(&package)
            .unwrap()
            .unwrap()
            .recent_error,
        None
    );
    registry.disconnect(&package).await.unwrap();
}

#[test]
fn connection_errors_map_to_stable_redacted_summaries() {
    let remote = ExternalPackageRemoteError::new(
        -32_001,
        "remote secret".to_owned(),
        Some(serde_json::json!({"api_key": "secret"})),
    );
    let cases = [
        (
            ExternalPackageConnectionError::Busy,
            "EXTERNAL_PACKAGE_BUSY",
        ),
        (
            ExternalPackageConnectionError::Timeout {
                request_id: "req-1".to_owned(),
                method: "hooks.upstream.frame".to_owned(),
            },
            "EXTERNAL_PACKAGE_TIMEOUT",
        ),
        (
            ExternalPackageConnectionError::Disconnected,
            "EXTERNAL_PACKAGE_DISCONNECTED",
        ),
        (
            ExternalPackageConnectionError::Remote {
                request_id: "req-2".to_owned(),
                method: "hooks.upstream.decode".to_owned(),
                error: remote,
            },
            "EXTERNAL_PACKAGE_REMOTE_ERROR",
        ),
        (
            ExternalPackageConnectionError::MessageTooLarge {
                actual_bytes: 2,
                limit_bytes: 1,
            },
            "EXTERNAL_PACKAGE_MESSAGE_TOO_LARGE",
        ),
        (
            ExternalPackageConnectionError::InvalidPayload("secret".to_owned()),
            "EXTERNAL_PACKAGE_INVALID_PAYLOAD",
        ),
        (
            ExternalPackageConnectionError::Fatal(
                ExternalPackageFatalProtocolError::InvalidResponse,
            ),
            "EXTERNAL_PACKAGE_PROTOCOL_FATAL",
        ),
        (
            ExternalPackageConnectionError::Transport("secret".to_owned()),
            "EXTERNAL_PACKAGE_TRANSPORT_ERROR",
        ),
    ];

    for (error, expected_code) in cases {
        let view = recent_error_view(&error);
        assert_eq!(view.code, expected_code);
        assert!(!view.message.contains("secret"));
    }
}

#[tokio::test]
async fn missing_persistent_row_rejects_connection_history_update() {
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    let (client, _peer) = connected_client(&registration, 50).await;
    let connection_id = ExternalPackageConnectionId(Uuid::new_v4());
    registry.online.lock().insert(
        package.clone(),
        OnlineConnection::Active {
            id: connection_id,
            client,
        },
    );
    registry.connection_details.lock().insert(
        package.clone(),
        ConnectionDetailSnapshot {
            connection_id,
            remote_address: None,
            rpc_timeout: Duration::from_secs(5),
            recent_error: None,
        },
    );

    let remote_error = registry
        .record_remote_address(&package, connection_id, "127.0.0.1:49001".parse().unwrap())
        .unwrap_err();
    let connection_error = registry
        .record_connection_error(
            &package,
            connection_id,
            &ExternalPackageConnectionError::Disconnected,
        )
        .unwrap_err();

    assert_eq!(remote_error.view_model.code, "PROTOCOL_PACKAGE_NOT_FOUND");
    assert_eq!(
        connection_error.view_model.code,
        "PROTOCOL_PACKAGE_NOT_FOUND"
    );
}
