use super::*;
use crate::sqlite::external_packages::registration_serialization_error;
use intercept_proxy_domain::ProtocolPackageRef;

fn insert_registration(store: &SqliteStore) -> (PackageManifest, ProtocolPackageRef) {
    let registration = registration("Vendor ISO8583");
    let package = registration.package().identity().clone();
    store
        .accept_external_package_registration(
            &registration,
            canonical_external_registration_fingerprint(&registration).unwrap(),
            Utc::now(),
        )
        .unwrap();
    (registration, package)
}

fn simulate_file_corruption(store: &SqliteStore, sql: &str) {
    let connection = store.connection.lock();
    connection
        .execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    connection.execute(sql, []).unwrap();
    connection
        .execute_batch("PRAGMA ignore_check_constraints = OFF")
        .unwrap();
}

#[test]
fn supplied_fingerprint_mismatch_is_rejected_before_insert() {
    let store = SqliteStore::in_memory().unwrap();
    let registration = registration("Vendor ISO8583");

    let error = store
        .accept_external_package_registration(&registration, [0_u8; 32], Utc::now())
        .unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
    assert!(store.list_external_packages().unwrap().is_empty());
}

#[test]
fn registration_serialization_failure_maps_to_redacted_corruption_error() {
    let serde_error = serde_json::from_str::<serde_json::Value>("{").unwrap_err();

    let error = registration_serialization_error(serde_error);

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
    assert!(!error.to_string().contains("secret"));
}

#[test]
fn indexed_identity_mismatch_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    store
        .connection
        .lock()
        .execute(
            "UPDATE external_protocol_packages SET package_id = 'different-id'",
            [],
        )
        .unwrap();
    let changed = ProtocolPackageRef {
        id: intercept_proxy_domain::ProtocolPackageId::new("different-id").unwrap(),
        version: package.version,
    };

    let error = store.get_external_package(&changed).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn indexed_version_mismatch_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    store
        .connection
        .lock()
        .execute(
            "UPDATE external_protocol_packages SET version = '2.0.0'",
            [],
        )
        .unwrap();
    let changed = ProtocolPackageRef {
        id: package.id,
        version: intercept_proxy_domain::ProtocolPackageVersion::new("2.0.0").unwrap(),
    };

    let error = store.get_external_package(&changed).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn malformed_registration_json_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    store
        .connection
        .lock()
        .execute(
            "UPDATE external_protocol_packages SET registration_json = '{'",
            [],
        )
        .unwrap();

    let error = store.get_external_package(&package).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn list_fails_closed_when_any_persisted_registration_is_corrupt() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, _package) = insert_registration(&store);
    store
        .connection
        .lock()
        .execute(
            "UPDATE external_protocol_packages SET registration_json = '{'",
            [],
        )
        .unwrap();

    let error = store.list_external_packages().unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn malformed_fingerprint_length_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    simulate_file_corruption(
        &store,
        "UPDATE external_protocol_packages SET registration_fingerprint = X'00'",
    );

    let error = store.get_external_package(&package).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn fingerprint_content_mismatch_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    store
        .connection
        .lock()
        .execute(
            "UPDATE external_protocol_packages SET registration_fingerprint = zeroblob(32)",
            [],
        )
        .unwrap();

    let error = store.get_external_package(&package).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn non_boolean_enabled_value_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    simulate_file_corruption(&store, "UPDATE external_protocol_packages SET enabled = 2");

    let error = store.get_external_package(&package).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn malformed_remote_address_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    store
        .connection
        .lock()
        .execute(
            "UPDATE external_protocol_packages SET last_remote_address = 'not-an-address'",
            [],
        )
        .unwrap();

    let error = store.get_external_package(&package).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn partial_recent_error_tuple_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    simulate_file_corruption(
        &store,
        "UPDATE external_protocol_packages SET recent_error_code = 'EXTERNAL_PACKAGE_BUSY'",
    );

    let error = store.get_external_package(&package).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn malformed_connection_timestamp_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    store
        .connection
        .lock()
        .execute(
            "UPDATE external_protocol_packages SET first_connected_at = 'not-a-timestamp'",
            [],
        )
        .unwrap();

    let error = store.get_external_package(&package).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn malformed_recent_error_timestamp_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    store
        .connection
        .lock()
        .execute(
            "UPDATE external_protocol_packages SET
                recent_error_code = 'EXTERNAL_PACKAGE_BUSY',
                recent_error_message = '外部软件包繁忙。',
                recent_error_occurred_at = 'not-a-timestamp'",
            [],
        )
        .unwrap();

    let error = store.get_external_package(&package).unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
}

#[test]
fn unknown_recent_error_code_is_rejected_without_mutation() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);

    let error = store
        .record_external_package_recent_error(
            &package,
            "EXTERNAL_PACKAGE_UNKNOWN",
            "外部软件包错误。",
            Utc::now(),
        )
        .unwrap_err();

    assert_eq!(
        error.code(),
        crate::InfrastructureErrorCode::PersistenceCorrupt
    );
    assert_eq!(
        store
            .get_external_package(&package)
            .unwrap()
            .unwrap()
            .recent_error,
        None
    );
}

#[test]
fn every_supported_safe_error_summary_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let (_registration, package) = insert_registration(&store);
    let cases = [
        ("EXTERNAL_PACKAGE_BUSY", "外部软件包繁忙。"),
        ("EXTERNAL_PACKAGE_TIMEOUT", "外部软件包调用超时。"),
        ("EXTERNAL_PACKAGE_DISCONNECTED", "外部软件包连接已断开。"),
        (
            "EXTERNAL_PACKAGE_REMOTE_ERROR",
            "外部软件包返回 JSON-RPC 错误。",
        ),
        (
            "EXTERNAL_PACKAGE_MESSAGE_TOO_LARGE",
            "外部软件包消息超过限制。",
        ),
        (
            "EXTERNAL_PACKAGE_INVALID_PAYLOAD",
            "外部软件包 payload 无效。",
        ),
        ("EXTERNAL_PACKAGE_PROTOCOL_FATAL", "外部软件包协议失效。"),
        ("EXTERNAL_PACKAGE_TRANSPORT_ERROR", "外部软件包传输失败。"),
        (
            "EXTERNAL_PACKAGE_PROCESS_FAILED",
            "本地软件包进程启动失败。",
        ),
    ];

    for (code, message) in cases {
        assert!(
            store
                .record_external_package_recent_error(&package, code, message, Utc::now())
                .unwrap()
        );
        let stored = store.get_external_package(&package).unwrap().unwrap();
        assert_eq!(stored.recent_error.unwrap().code, code);
    }
}
