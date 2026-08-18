use rusqlite::Connection;

use super::*;
use crate::sqlite::protocol_packages::{
    StoredProtocolPackageValidation, protocol_package_preflight_error_code,
};

#[test]
fn empty_database_creates_current_schema_once_and_reopen_is_read_only() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    let store = SqliteStore::open(&path).unwrap();
    assert_protocol_tables_and_current_marker(&store);
    drop(store);

    let reopened = SqliteStore::open(&path).unwrap();
    assert_protocol_tables_and_current_marker(&reopened);
}

#[test]
fn database_without_current_marker_is_rejected_without_adding_tables() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE settings (
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

    assert!(matches!(
        SqliteStore::open(&path),
        Err(InfrastructureError::DatabaseSchemaInvalid { found, .. }) if found.is_empty()
    ));
    let connection = Connection::open(&path).unwrap();
    let protocol_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name IN ('protocol_packages', 'protocol_package_files')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(protocol_tables, 0);
}

#[test]
fn old_or_unknown_schema_marker_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    let store = SqliteStore::open(&path).unwrap();
    store
        .connection
        .lock()
        .execute("UPDATE application_schema SET version = 9", [])
        .unwrap();
    drop(store);
    assert!(matches!(
        SqliteStore::open(&path),
        Err(InfrastructureError::DatabaseSchemaInvalid { .. })
    ));

    let connection = Connection::open(&path).unwrap();
    connection
        .execute("UPDATE application_schema SET version = 999", [])
        .unwrap();
    drop(connection);
    assert!(matches!(
        SqliteStore::open(&path),
        Err(InfrastructureError::DatabaseSchemaInvalid { .. })
    ));
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

fn assert_protocol_tables_and_current_marker(store: &SqliteStore) {
    let tables = store.table_names().unwrap();
    assert!(tables.contains(&"protocol_packages".to_owned()));
    assert!(tables.contains(&"protocol_package_files".to_owned()));
    let marker: (i64, i64) = store
        .connection
        .lock()
        .query_row(
            "SELECT singleton_id, version FROM application_schema",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(marker, (1, 10));
}
