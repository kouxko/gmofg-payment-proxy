use rusqlite::Connection;

use super::*;
use crate::sqlite::protocol_packages::{
    StoredProtocolPackageValidation, protocol_package_preflight_error_code,
};

#[test]
fn empty_database_records_protocol_package_schema_once_and_reopen_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    let store = SqliteStore::open(&path).unwrap();
    assert_protocol_tables_and_version(&store, 1);
    drop(store);

    let reopened = SqliteStore::open(&path).unwrap();
    assert_protocol_tables_and_version(&reopened, 1);
}

#[test]
fn version_five_database_upgrades_without_losing_old_rows() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE schema_migrations (
                 version INTEGER PRIMARY KEY,
                 applied_at TEXT NOT NULL
             );
             INSERT INTO schema_migrations(version, applied_at) VALUES (5, '2026-08-13T00:00:00Z');
             CREATE TABLE settings (
                 singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                 revision INTEGER NOT NULL,
                 json TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             INSERT INTO settings(singleton_id, revision, json, updated_at)
             VALUES (1, 7, '{\"kept\":true}', '2026-08-13T00:00:00Z');",
        )
        .unwrap();
    drop(connection);

    let store = SqliteStore::open(&path).unwrap();
    assert_protocol_tables_and_version(&store, 1);
    assert_eq!(
        store.load_settings().unwrap().unwrap().value,
        serde_json::json!({"kept": true})
    );
    let old_version: i64 = store
        .connection
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 5",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(old_version, 1);
}

#[test]
fn failed_protocol_schema_migration_rolls_back_new_tables_and_version_record() {
    let store = SqliteStore::in_memory().unwrap();
    {
        let connection = store.connection.lock();
        connection
            .execute_batch(
                "DROP TABLE protocol_package_files;
                 DROP TABLE protocol_packages;
                 DELETE FROM schema_migrations WHERE version = 10;
                 CREATE TRIGGER reject_protocol_migration
                 BEFORE INSERT ON schema_migrations
                 WHEN NEW.version = 10
                 BEGIN SELECT RAISE(ABORT, 'reject protocol migration'); END;",
            )
            .unwrap();
    }

    assert!(matches!(
        store.migrate(),
        Err(InfrastructureError::DatabaseMigration { .. })
    ));
    let tables = store.table_names().unwrap();
    assert!(!tables.contains(&"protocol_packages".to_owned()));
    assert!(!tables.contains(&"protocol_package_files".to_owned()));
    let version: i64 = store
        .connection
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 10",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 0);
}

#[test]
fn corrupt_protocol_header_is_isolated_and_enabled_fails_closed() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .connection
        .lock()
        .execute(
            "INSERT INTO protocol_packages(
                package_id, version, name, host_api, enabled,
                validation_state, validation_error_code, installed_at, generation
             ) VALUES ('example-protocol', '1.0.0', 'name', -1, 0, 'valid', NULL, ?1, ?2)",
            [Utc::now().to_rfc3339(), uuid::Uuid::new_v4().to_string()],
        )
        .unwrap();

    let headers = store.list_protocol_package_headers().unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].host_api, 0);
    assert!(!headers[0].enabled);
    assert_eq!(
        headers[0].validation,
        StoredProtocolPackageValidation::Invalid("PERSISTENCE_CORRUPT".to_owned())
    );

    let connection = store.connection.lock();
    let invalid_enabled = connection.execute(
        "UPDATE protocol_packages SET enabled = 2
         WHERE package_id = 'example-protocol' AND version = '1.0.0'",
        [],
    );
    assert!(
        invalid_enabled.is_err(),
        "schema must reject non-boolean state"
    );
    connection
        .execute_batch("PRAGMA ignore_check_constraints = ON")
        .unwrap();
    connection
        .execute(
            "UPDATE protocol_packages SET enabled = 2
             WHERE package_id = 'example-protocol' AND version = '1.0.0'",
            [],
        )
        .unwrap();
    connection
        .execute_batch("PRAGMA ignore_check_constraints = OFF")
        .unwrap();
    drop(connection);

    let header = store.list_protocol_package_headers().unwrap().remove(0);
    assert!(
        !header.enabled,
        "corrupt integer must never enable a package"
    );
    assert_eq!(
        header.validation,
        StoredProtocolPackageValidation::Invalid("PERSISTENCE_CORRUPT".to_owned())
    );
}

#[test]
fn remaining_corrupt_header_metadata_is_safely_quarantined() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .connection
        .lock()
        .execute(
            "INSERT INTO protocol_packages(
                package_id, version, name, host_api, enabled,
                validation_state, validation_error_code, installed_at, generation
             ) VALUES ('example-protocol', '1.0.0', '', 1, 0, 'invalid',
                       'not_safe', 'not-a-time', 'not-a-generation')",
            [],
        )
        .unwrap();

    let header = store.list_protocol_package_headers().unwrap().remove(0);
    assert_eq!(header.name, "Invalid protocol package");
    assert_eq!(header.installed_at, DateTime::<Utc>::UNIX_EPOCH);
    assert_eq!(header.generation, uuid::Uuid::nil());
    assert_eq!(
        header.validation,
        StoredProtocolPackageValidation::Invalid("PERSISTENCE_CORRUPT".to_owned())
    );
}

#[test]
fn unidentifiable_header_returns_a_global_safe_persistence_error() {
    for (id, version) in [("bad id", "1.0.0"), ("example-protocol", "bad version")] {
        let store = SqliteStore::in_memory().unwrap();
        store
            .connection
            .lock()
            .execute(
                "INSERT INTO protocol_packages(
                    package_id, version, name, host_api, enabled,
                    validation_state, validation_error_code, installed_at, generation
                 ) VALUES (?1, ?2, 'name', 1, 0, 'valid', NULL, ?3, ?4)",
                rusqlite::params![
                    id,
                    version,
                    Utc::now().to_rfc3339(),
                    uuid::Uuid::new_v4().to_string(),
                ],
            )
            .unwrap();

        let error = store.list_protocol_package_headers().unwrap_err();
        assert_eq!(
            error.code(),
            crate::InfrastructureErrorCode::PersistenceCorrupt
        );
        assert!(!error.to_string().contains(id));
        assert!(!error.to_string().contains(version));
    }
}

#[test]
fn corrupted_file_aggregates_are_rejected_before_blob_loading() {
    let max_entries =
        i64::try_from(intercept_proxy_protocol_scripting::MAX_ARCHIVE_ENTRIES_LIMIT).unwrap();
    let max_path =
        i64::try_from(intercept_proxy_protocol_scripting::MAX_PACKAGE_FILE_PATH_BYTES).unwrap();
    let max_file = i64::try_from(intercept_proxy_protocol_scripting::MAX_FILE_BYTES_LIMIT).unwrap();
    let max_total =
        i64::try_from(intercept_proxy_protocol_scripting::MAX_TOTAL_BYTES_LIMIT).unwrap();
    for (metrics, expected) in [
        ((-1, 1, 1, 1), "TOO_MANY_ENTRIES"),
        ((max_entries + 1, 1, 1, 1), "TOO_MANY_ENTRIES"),
        ((1, -1, 1, 1), "INVALID_PATH"),
        ((1, max_path + 1, 1, 1), "INVALID_PATH"),
        ((1, 1, -1, 1), "FILE_TOO_LARGE"),
        ((1, 1, max_file + 1, 1), "FILE_TOO_LARGE"),
        ((1, 1, 1, -1), "TOTAL_TOO_LARGE"),
        ((1, 1, 1, max_total + 1), "TOTAL_TOO_LARGE"),
    ] {
        assert_eq!(
            protocol_package_preflight_error_code(metrics.0, metrics.1, metrics.2, metrics.3),
            Some(expected)
        );
    }
    assert_eq!(
        protocol_package_preflight_error_code(max_entries, max_path, max_file, max_total),
        None
    );
}

fn assert_protocol_tables_and_version(store: &SqliteStore, expected_latest_rows: i64) {
    let tables = store.table_names().unwrap();
    assert!(tables.contains(&"protocol_packages".to_owned()));
    assert!(tables.contains(&"protocol_package_files".to_owned()));
    let version: i64 = store
        .connection
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 10",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, expected_latest_rows);
}
