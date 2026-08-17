use chrono::{Duration, TimeZone, Utc};
use serde_json::json;
use tempfile::tempdir;
use uuid::Uuid;

use super::*;
use crate::sqlite::socket_captures::{
    SocketCaptureInsert, SocketCaptureRetention, SocketCaptureStorageQuery,
    SocketCaptureStorageSort, SocketCaptureStorageSortDirection, SocketCaptureStoreError,
    StoredSocketCaptureKind,
};

fn capture(
    capture_id: u128,
    workspace_id: Uuid,
    listener_id: Uuid,
    connection_id: Uuid,
    kind: StoredSocketCaptureKind,
) -> SocketCaptureInsert {
    let capture_number = i64::try_from(capture_id).unwrap();
    let occurred_at = Utc.timestamp_opt(capture_number, 0).unwrap();
    SocketCaptureInsert {
        capture_id: Uuid::from_u128(capture_id),
        runtime_epoch: Uuid::from_u128(100),
        workspace_id,
        listener_id,
        session_id: Uuid::from_u128(capture_id + 1_000),
        connection_id,
        occurred_at,
        completed_at: occurred_at + Duration::milliseconds(capture_number),
        kind,
        direction: (kind == StoredSocketCaptureKind::RelayFrame)
            .then(|| "upstream_receive".to_owned()),
        package_id: "example-protocol".to_owned(),
        package_version: "1.0.0".to_owned(),
        logical_bytes: 512,
        payload: json!({"capture_id": capture_id, "origin": [1, 2, 3]}),
    }
}

fn query() -> SocketCaptureStorageQuery {
    SocketCaptureStorageQuery {
        workspace_id: None,
        listener_id: None,
        session_id: None,
        connection_id: None,
        package: None,
        kind: None,
        direction: None,
        occurred_from: None,
        occurred_to: None,
        sort: SocketCaptureStorageSort::OccurredAt,
        sort_direction: SocketCaptureStorageSortDirection::Desc,
        page: 1,
        page_size: 20,
    }
}

#[test]
fn socket_capture_round_trip_filters_pages_and_preserves_full_metadata() {
    let store = SqliteStore::in_memory().expect("store");
    let workspace = Uuid::new_v4();
    let other_workspace = Uuid::new_v4();
    let listener = Uuid::new_v4();
    let other_listener = Uuid::new_v4();
    let connection = Uuid::new_v4();
    let relay = capture(
        1,
        workspace,
        listener,
        connection,
        StoredSocketCaptureKind::RelayFrame,
    );
    let local = capture(
        2,
        workspace,
        other_listener,
        Uuid::new_v4(),
        StoredSocketCaptureKind::LocalExchange,
    );
    let foreign = capture(
        3,
        other_workspace,
        listener,
        connection,
        StoredSocketCaptureKind::RelayFrame,
    );
    for value in [&relay, &local, &foreign] {
        store.insert_socket_capture(value).expect("insert capture");
    }

    let mut page_query = query();
    page_query.workspace_id = Some(workspace);
    page_query.page_size = 1;
    let first = store
        .query_socket_captures(&page_query)
        .expect("first page");
    assert_eq!((first.total, first.total_pages), (2, 2));
    assert_eq!(first.rows[0].capture, local);
    page_query.page = 2;
    assert_eq!(
        store
            .query_socket_captures(&page_query)
            .expect("second page")
            .rows[0]
            .capture,
        relay
    );

    let mut filtered = query();
    filtered.workspace_id = Some(workspace);
    filtered.listener_id = Some(listener);
    filtered.session_id = Some(relay.session_id);
    filtered.connection_id = Some(connection);
    filtered.package = Some((relay.package_id.clone(), relay.package_version.clone()));
    filtered.kind = Some(StoredSocketCaptureKind::RelayFrame);
    filtered.direction = Some("upstream_receive".to_owned());
    filtered.occurred_from = Some(relay.occurred_at);
    filtered.occurred_to = Some(relay.occurred_at);
    assert_eq!(
        store
            .query_socket_captures(&filtered)
            .expect("filtered")
            .rows,
        vec![
            store
                .get_socket_capture(relay.capture_id)
                .expect("detail")
                .expect("stored")
        ]
    );
}

#[test]
fn all_sorts_have_sequence_tie_breaker_and_normalized_pagination() {
    let store = SqliteStore::in_memory().expect("store");
    let workspace = Uuid::new_v4();
    let listener = Uuid::new_v4();
    let connection = Uuid::new_v4();
    let mut first = capture(
        1,
        workspace,
        listener,
        connection,
        StoredSocketCaptureKind::RelayFrame,
    );
    let mut second = capture(
        2,
        workspace,
        listener,
        connection,
        StoredSocketCaptureKind::RelayFrame,
    );
    second.occurred_at = first.occurred_at;
    second.completed_at = first.completed_at;
    first.logical_bytes = 256;
    second.logical_bytes = 256;
    store.insert_socket_capture(&first).unwrap();
    store.insert_socket_capture(&second).unwrap();

    for sort in [
        SocketCaptureStorageSort::OccurredAt,
        SocketCaptureStorageSort::CompletedAt,
        SocketCaptureStorageSort::Size,
    ] {
        let mut request = query();
        request.sort = sort;
        request.page = 0;
        request.page_size = 0;
        let descending = store.query_socket_captures(&request).unwrap();
        assert_eq!((descending.page, descending.page_size), (1, 1));
        assert_eq!(descending.rows[0].capture.capture_id, second.capture_id);
        request.sort_direction = SocketCaptureStorageSortDirection::Asc;
        assert_eq!(
            store.query_socket_captures(&request).unwrap().rows[0]
                .capture
                .capture_id,
            first.capture_id
        );
    }
}

#[test]
fn count_and_logical_byte_retention_delete_oldest_sequence() {
    let store = SqliteStore::in_memory().expect("store");
    let workspace = Uuid::new_v4();
    let listener = Uuid::new_v4();
    let connection = Uuid::new_v4();
    let count_limit = SocketCaptureRetention {
        max_records: 2,
        max_logical_bytes: 64 * 1024,
    };
    for id in 1..=3 {
        store
            .insert_socket_capture_with_retention(
                &capture(
                    id,
                    workspace,
                    listener,
                    connection,
                    StoredSocketCaptureKind::RelayFrame,
                ),
                count_limit,
            )
            .expect("bounded insert");
    }
    let retained = store.query_socket_captures(&query()).expect("retained");
    assert_eq!(
        retained
            .rows
            .iter()
            .map(|row| row.capture.capture_id)
            .collect::<Vec<_>>(),
        vec![Uuid::from_u128(3), Uuid::from_u128(2)]
    );

    store.clear_socket_captures(None).expect("clear rows");
    let byte_limit = SocketCaptureRetention {
        max_records: 10,
        max_logical_bytes: 1_025,
    };
    for id in 10..=12 {
        store
            .insert_socket_capture_with_retention(
                &capture(
                    id,
                    workspace,
                    listener,
                    connection,
                    StoredSocketCaptureKind::LocalExchange,
                ),
                byte_limit,
            )
            .expect("byte bounded insert");
    }
    let retained = store
        .query_socket_captures(&query())
        .expect("byte retained");
    assert_eq!(retained.total, 2);
    assert_eq!(retained.rows[0].capture.capture_id, Uuid::from_u128(12));
    assert_eq!(retained.rows[1].capture.capture_id, Uuid::from_u128(11));
}

#[test]
fn invalid_shapes_oversize_and_corrupt_rows_fail_closed() {
    let store = SqliteStore::in_memory().expect("store");
    let workspace = Uuid::new_v4();
    let listener = Uuid::new_v4();
    let connection = Uuid::new_v4();
    let mut invalid = capture(
        1,
        workspace,
        listener,
        connection,
        StoredSocketCaptureKind::LocalExchange,
    );
    invalid.direction = Some("downstream_send".to_owned());
    assert!(matches!(
        store.insert_socket_capture(&invalid),
        Err(SocketCaptureStoreError::InvalidRecord { .. })
    ));

    let valid = capture(
        2,
        workspace,
        listener,
        connection,
        StoredSocketCaptureKind::RelayFrame,
    );
    assert!(matches!(
        store.insert_socket_capture_with_retention(
            &valid,
            SocketCaptureRetention {
                max_records: 1,
                max_logical_bytes: 1,
            },
        ),
        Err(SocketCaptureStoreError::PayloadTooLarge { .. })
    ));

    store.insert_socket_capture(&valid).expect("valid insert");
    {
        let connection = store.connection.lock();
        connection
            .execute_batch("PRAGMA ignore_check_constraints = ON;")
            .unwrap();
        connection
            .execute(
                "UPDATE socket_captures SET payload_json = 'not-json' WHERE capture_id = ?1",
                [valid.capture_id.to_string()],
            )
            .unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints = OFF;")
            .unwrap();
    }
    let error = store
        .query_socket_captures(&query())
        .expect_err("corruption must fail closed");
    assert!(matches!(
        error,
        SocketCaptureStoreError::Infrastructure(InfrastructureError::PersistenceCorrupt {
            entity: "socket_capture",
            ..
        })
    ));
}

#[test]
fn storage_debug_never_contains_payload_bytes_html_or_document_values() {
    let workspace = Uuid::new_v4();
    let mut value = capture(
        1,
        workspace,
        Uuid::new_v4(),
        Uuid::new_v4(),
        StoredSocketCaptureKind::RelayFrame,
    );
    value.payload = json!({
        "origin": "SECRET_FRAME_123",
        "display": "<p>SECRET_HTML_456</p>",
        "document": {"pan": "SECRET_DOCUMENT_789"}
    });
    let debug = format!("{value:?}");
    for secret in ["SECRET_FRAME_123", "SECRET_HTML_456", "SECRET_DOCUMENT_789"] {
        assert!(!debug.contains(secret));
    }
    assert!(debug.contains("payload_object_fields"));
    assert!(debug.contains(&workspace.to_string()));
}

#[test]
fn captures_recover_after_reopen_and_clear_is_workspace_scoped() {
    let directory = tempdir().expect("tempdir");
    let path = directory.path().join("captures.sqlite3");
    let workspace = Uuid::new_v4();
    let other_workspace = Uuid::new_v4();
    let listener = Uuid::new_v4();
    let connection = Uuid::new_v4();
    let capture_id = Uuid::from_u128(1);
    {
        let store = SqliteStore::open(&path).expect("first open");
        for value in [
            capture(
                1,
                workspace,
                listener,
                connection,
                StoredSocketCaptureKind::RelayFrame,
            ),
            capture(
                2,
                other_workspace,
                listener,
                connection,
                StoredSocketCaptureKind::RelayFrame,
            ),
        ] {
            store.insert_socket_capture(&value).expect("insert");
        }
    }
    let store = SqliteStore::open(&path).expect("reopen");
    assert!(store.get_socket_capture(capture_id).unwrap().is_some());
    assert_eq!(store.clear_socket_captures(Some(workspace)).unwrap(), 1);
    let mut workspace_query = query();
    workspace_query.workspace_id = Some(workspace);
    assert_eq!(
        store.query_socket_captures(&workspace_query).unwrap().total,
        0
    );
    workspace_query.workspace_id = Some(other_workspace);
    assert_eq!(
        store.query_socket_captures(&workspace_query).unwrap().total,
        1
    );
}

#[test]
fn version_six_database_migrates_to_socket_capture_schema_without_losing_history() {
    let directory = tempdir().expect("tempdir");
    let path = directory.path().join("version-six.sqlite3");
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL
                 );
                 INSERT INTO schema_migrations(version, applied_at)
                 VALUES (6, '2026-08-14T00:00:00Z');",
            )
            .unwrap();
    }
    let store = SqliteStore::open(&path).expect("migrate v6");
    let connection = store.connection.lock();
    let versions: Vec<i64> = connection
        .prepare("SELECT version FROM schema_migrations ORDER BY version")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(versions, vec![6, 9]);
    let table_exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'socket_captures'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_exists, 1);
}

#[test]
fn configuration_replace_preserves_captures_but_explicit_application_reset_deletes_them() {
    let store = SqliteStore::in_memory().expect("store");
    let capture_workspace = Uuid::new_v4();
    let stored = capture(
        1,
        capture_workspace,
        Uuid::new_v4(),
        Uuid::new_v4(),
        StoredSocketCaptureKind::RelayFrame,
    );
    store.insert_socket_capture(&stored).expect("seed capture");

    let first_id = Uuid::new_v4();
    let first = WorkspaceRecord {
        id: first_id,
        revision: 1,
        value: json!({"id": first_id, "name": "replacement", "revision": 1}),
        updated_at: Utc::now(),
    };
    store
        .replace_application_configuration(
            first_id,
            std::slice::from_ref(&first),
            &json!({"generation": 1}),
        )
        .expect("ordinary replace");
    assert!(
        store
            .get_socket_capture(stored.capture_id)
            .unwrap()
            .is_some()
    );

    let second_id = Uuid::new_v4();
    let second = WorkspaceRecord {
        id: second_id,
        revision: 1,
        value: json!({"id": second_id, "name": "portable", "revision": 1}),
        updated_at: Utc::now(),
    };
    store
        .replace_application_bundle(
            second_id,
            std::slice::from_ref(&second),
            &json!({"generation": 2}),
            &[],
        )
        .expect("portable replace");
    assert!(
        store
            .get_socket_capture(stored.capture_id)
            .unwrap()
            .is_some()
    );

    store
        .reset_application_bundle(
            second_id,
            std::slice::from_ref(&second),
            &json!({"generation": 3}),
        )
        .expect("explicit reset");
    assert!(
        store
            .get_socket_capture(stored.capture_id)
            .unwrap()
            .is_none()
    );
}
