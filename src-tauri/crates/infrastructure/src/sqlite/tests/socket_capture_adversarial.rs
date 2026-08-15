use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

use crate::SqliteStore;
use crate::sqlite::socket_captures::{
    SocketCaptureInsert, SocketCaptureStorageQuery, SocketCaptureStorageSort,
    SocketCaptureStorageSortDirection, StoredSocketCaptureKind,
};

fn capture(id: u128, workspace_id: Uuid) -> SocketCaptureInsert {
    let occurred_at = Utc.timestamp_opt(i64::try_from(id).unwrap(), 0).unwrap();
    SocketCaptureInsert {
        capture_id: Uuid::from_u128(id),
        runtime_epoch: Uuid::from_u128(100),
        workspace_id,
        listener_id: Uuid::from_u128(101),
        session_id: Uuid::from_u128(102),
        connection_id: Uuid::from_u128(103),
        occurred_at,
        completed_at: occurred_at,
        kind: StoredSocketCaptureKind::RelayFrame,
        direction: Some("upstream".to_owned()),
        package_id: "iso8583".to_owned(),
        package_version: "1.0.0".to_owned(),
        logical_bytes: u64::try_from(id).unwrap() + 1,
        payload: json!({"capture": id}),
    }
}

fn query(workspace_id: Uuid, page: u32, page_size: u32) -> SocketCaptureStorageQuery {
    SocketCaptureStorageQuery {
        workspace_id: Some(workspace_id),
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
        page,
        page_size,
    }
}

#[test]
fn queued_generation_cannot_resurrect_after_scoped_clear_or_full_reset() {
    let store = SqliteStore::in_memory().unwrap();
    let first_workspace = Uuid::from_u128(1_000);
    let second_workspace = Uuid::from_u128(2_000);
    let stale_scoped = capture(1, first_workspace);
    let unaffected = capture(2, second_workspace);
    let scoped_ticket = store.socket_capture_generation(first_workspace);
    let unaffected_ticket = store.socket_capture_generation(second_workspace);

    assert_eq!(
        store.clear_socket_captures(Some(first_workspace)).unwrap(),
        0
    );
    assert!(
        store
            .insert_socket_capture_if_current(&stale_scoped, &scoped_ticket)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .insert_socket_capture_if_current(&unaffected, &unaffected_ticket)
            .unwrap()
            .is_some()
    );

    let stale_reset = capture(3, second_workspace);
    let reset_ticket = store.socket_capture_generation(second_workspace);
    assert_eq!(store.clear_socket_captures(None).unwrap(), 1);
    assert!(
        store
            .insert_socket_capture_if_current(&stale_reset, &reset_ticket)
            .unwrap()
            .is_none()
    );
}

#[test]
fn sql_page_deserializes_only_selected_rows_and_reports_selected_corruption() {
    let store = SqliteStore::in_memory().unwrap();
    let workspace = Uuid::from_u128(4_000);
    for id in 1..=3 {
        store
            .insert_socket_capture(&capture(id, workspace))
            .unwrap();
    }
    store
        .connection
        .lock()
        .execute(
            "UPDATE socket_captures SET runtime_epoch = 'damaged' WHERE capture_id = ?1",
            [Uuid::from_u128(1).to_string()],
        )
        .unwrap();

    let first_page = store
        .query_socket_captures(&query(workspace, 1, 1))
        .unwrap();
    assert_eq!(first_page.total, 3);
    assert_eq!(first_page.rows.len(), 1);
    assert_eq!(first_page.rows[0].capture.capture_id, Uuid::from_u128(3));

    let selected_corruption = store
        .query_socket_captures(&query(workspace, 3, 1))
        .unwrap_err();
    assert!(matches!(
        selected_corruption,
        crate::sqlite::socket_captures::SocketCaptureStoreError::Infrastructure(
            crate::InfrastructureError::PersistenceCorrupt { .. }
        )
    ));
}
