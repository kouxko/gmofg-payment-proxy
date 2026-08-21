use chrono::Utc;
use intercept_proxy_domain::ExternalPackageRegistration;
use std::net::SocketAddr;

use super::*;
use crate::sqlite::external_packages::StoredExternalPackageRegistrationOutcome;
use crate::sqlite::external_packages::canonical_external_registration_fingerprint;

fn registration(name: &str) -> ExternalPackageRegistration {
    serde_json::from_value(serde_json::json!({
        "api": 1,
        "package": {
            "id": "vendor-iso8583",
            "name": name,
            "version": "1.0.0",
            "description": "external test package"
        },
        "document": {
            "upstream": {
                "schema": {
                    "id": "vendor-upstream", "version": 1, "title": "Upstream",
                    "fields": [{"name": "mti", "label": "MTI", "type": "string"}]
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

#[test]
fn first_registration_is_disabled_and_reconnect_preserves_enabled() {
    let store = SqliteStore::in_memory().unwrap();
    let package = registration("Vendor ISO8583");
    let identity = package.package().identity().clone();
    let fingerprint = canonical_external_registration_fingerprint(&package).unwrap();
    let first_connected_at = Utc::now();

    assert_eq!(
        store
            .accept_external_package_registration(&package, fingerprint, first_connected_at)
            .unwrap(),
        StoredExternalPackageRegistrationOutcome::Inserted
    );
    let first = store
        .get_external_package(&identity)
        .unwrap()
        .expect("stored package");
    assert!(!first.enabled);
    assert_eq!(first.first_connected_at, first_connected_at);
    assert_eq!(first.last_connected_at, first_connected_at);
    assert_eq!(first.remote_address, None);
    assert_eq!(first.recent_error, None);

    assert!(store.set_external_package_enabled(&identity, true).unwrap());
    let reconnected_at = first_connected_at + chrono::Duration::seconds(1);
    assert_eq!(
        store
            .accept_external_package_registration(&package, fingerprint, reconnected_at)
            .unwrap(),
        StoredExternalPackageRegistrationOutcome::Reconnected { enabled: true }
    );
    let reconnected = store
        .get_external_package(&identity)
        .unwrap()
        .expect("stored package");
    assert!(reconnected.enabled);
    assert_eq!(reconnected.first_connected_at, first_connected_at);
    assert_eq!(reconnected.last_connected_at, reconnected_at);
}

#[test]
fn connection_address_and_safe_recent_error_survive_store_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("external-packages.sqlite3");
    let package = registration("Vendor ISO8583");
    let identity = package.package().identity().clone();
    let remote_address: SocketAddr = "127.0.0.1:49152".parse().unwrap();
    let error_at = Utc::now();

    {
        let store = SqliteStore::open(&database).unwrap();
        store
            .accept_external_package_registration(
                &package,
                canonical_external_registration_fingerprint(&package).unwrap(),
                error_at - chrono::Duration::seconds(1),
            )
            .unwrap();
        assert!(
            store
                .record_external_package_remote_address(&identity, remote_address)
                .unwrap()
        );
        assert!(
            store
                .record_external_package_recent_error(
                    &identity,
                    "EXTERNAL_PACKAGE_DISCONNECTED",
                    "外部软件包连接已断开。",
                    error_at,
                )
                .unwrap()
        );
    }

    let reopened = SqliteStore::open(&database).unwrap();
    let stored = reopened
        .get_external_package(&identity)
        .unwrap()
        .expect("persisted package");
    assert_eq!(stored.remote_address, Some(remote_address));
    let recent_error = stored.recent_error.expect("persisted safe error");
    assert_eq!(recent_error.code, "EXTERNAL_PACKAGE_DISCONNECTED");
    assert_eq!(recent_error.message, "外部软件包连接已断开。");
    assert_eq!(recent_error.occurred_at, error_at);
}

#[test]
fn recording_a_new_connection_atomically_clears_the_previous_error() {
    let store = SqliteStore::in_memory().unwrap();
    let package = registration("Vendor ISO8583");
    let identity = package.package().identity().clone();
    let connected_at = Utc::now();
    store
        .accept_external_package_registration(
            &package,
            canonical_external_registration_fingerprint(&package).unwrap(),
            connected_at,
        )
        .unwrap();
    store
        .record_external_package_recent_error(
            &identity,
            "EXTERNAL_PACKAGE_TRANSPORT_ERROR",
            "外部软件包传输失败。",
            connected_at,
        )
        .unwrap();

    let next_remote: SocketAddr = "[::1]:49153".parse().unwrap();
    assert!(
        store
            .record_external_package_remote_address(&identity, next_remote)
            .unwrap()
    );

    let stored = store
        .get_external_package(&identity)
        .unwrap()
        .expect("stored package");
    assert_eq!(stored.remote_address, Some(next_remote));
    assert_eq!(stored.recent_error, None);
}

#[test]
fn recent_error_rejects_dynamic_remote_text_without_mutating_storage() {
    let store = SqliteStore::in_memory().unwrap();
    let package = registration("Vendor ISO8583");
    let identity = package.package().identity().clone();
    store
        .accept_external_package_registration(
            &package,
            canonical_external_registration_fingerprint(&package).unwrap(),
            Utc::now(),
        )
        .unwrap();

    let error = store
        .record_external_package_recent_error(
            &identity,
            "EXTERNAL_PACKAGE_REMOTE_ERROR",
            "remote data contains api_key=secret",
            Utc::now(),
        )
        .expect_err("dynamic remote content must not be persisted");

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
    assert_eq!(
        store
            .get_external_package(&identity)
            .unwrap()
            .expect("stored package")
            .recent_error,
        None
    );
}

#[test]
fn exact_identity_with_changed_registration_is_rejected_without_mutation() {
    let store = SqliteStore::in_memory().unwrap();
    let original = registration("Original");
    let changed = registration("Changed");
    let identity = original.package().identity().clone();
    let first_connected_at = Utc::now();
    store
        .accept_external_package_registration(
            &original,
            canonical_external_registration_fingerprint(&original).unwrap(),
            first_connected_at,
        )
        .unwrap();

    assert_eq!(
        store
            .accept_external_package_registration(
                &changed,
                canonical_external_registration_fingerprint(&changed).unwrap(),
                first_connected_at + chrono::Duration::seconds(1),
            )
            .unwrap(),
        StoredExternalPackageRegistrationOutcome::IdentityConflict
    );
    let stored = store
        .get_external_package(&identity)
        .unwrap()
        .expect("original package remains");
    assert_eq!(stored.registration, original);
    assert_eq!(
        stored.fingerprint,
        canonical_external_registration_fingerprint(&original).unwrap()
    );
    assert_eq!(stored.last_connected_at, first_connected_at);
}

#[test]
fn list_and_delete_use_exact_package_version() {
    let store = SqliteStore::in_memory().unwrap();
    let package = registration("Vendor ISO8583");
    let identity = package.package().identity().clone();
    store
        .accept_external_package_registration(
            &package,
            canonical_external_registration_fingerprint(&package).unwrap(),
            Utc::now(),
        )
        .unwrap();

    assert_eq!(store.list_external_packages().unwrap().len(), 1);
    assert!(store.delete_external_package(&identity).unwrap());
    assert!(!store.delete_external_package(&identity).unwrap());
    assert!(store.list_external_packages().unwrap().is_empty());
}

#[test]
fn schema_persists_only_registration_state_and_never_rpc_or_secret_payloads() {
    let store = SqliteStore::in_memory().unwrap();
    let columns = store
        .connection
        .lock()
        .prepare("PRAGMA table_info(external_protocol_packages)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap();
    assert_eq!(
        columns,
        [
            "package_id",
            "version",
            "registration_json",
            "registration_fingerprint",
            "enabled",
            "first_connected_at",
            "last_connected_at",
            "last_remote_address",
            "recent_error_code",
            "recent_error_message",
            "recent_error_occurred_at",
        ]
    );
    assert!(!columns.iter().any(|column| {
        column.contains("request")
            || column.contains("response")
            || column.contains("secret")
            || column.contains("key")
    }));
}

#[test]
fn internal_exact_identity_blocks_external_registration_without_partial_write() {
    let store = SqliteStore::in_memory().unwrap();
    let external = registration("Vendor ISO8583");
    let identity = external.package().identity();
    store
        .connection
        .lock()
        .execute(
            "INSERT INTO protocol_packages(
                package_id, version, name, host_api, kind, enabled,
                validation_state, validation_error_code, installed_at, generation
             ) VALUES (?1, ?2, 'Internal package', 1, 'socket', 0,
                       'valid', NULL, ?3, ?4)",
            rusqlite::params![
                identity.id.as_str(),
                identity.version.as_str(),
                Utc::now().to_rfc3339(),
                uuid::Uuid::new_v4().to_string(),
            ],
        )
        .unwrap();

    assert_eq!(
        store
            .accept_external_package_registration(
                &external,
                canonical_external_registration_fingerprint(&external).unwrap(),
                Utc::now(),
            )
            .unwrap(),
        StoredExternalPackageRegistrationOutcome::IdentityConflict
    );
    assert!(store.list_external_packages().unwrap().is_empty());
}

mod corruption;
