use chrono::{Duration, TimeZone, Utc};
use intercept_proxy_application::{
    CaptureQuery, CaptureRepositoryPort, CaptureSort, PageRequest, SocketCaptureSchemaRef,
    SocketCaptureSort, SocketDisplayFallbackReason, SocketDisplayResult, SocketRelayFrameCapture,
    SocketWriteKind,
};
use intercept_proxy_domain::{
    DocumentSchemaId, ListenerId, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    SocketDirection, WorkspaceId,
};
use uuid::Uuid;

use super::*;
use crate::adapters::CaptureRepositoryAdapter;

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("iso8583").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn record(id: u128, workspace_id: WorkspaceId) -> SocketCaptureRecord {
    let second = u32::try_from(id).unwrap();
    let milliseconds = i64::try_from(id).unwrap();
    let byte = u8::try_from(id).unwrap();
    let occurred_at = Utc.with_ymd_and_hms(2026, 8, 15, 10, 0, second).unwrap();
    SocketCaptureRecord {
        capture_id: intercept_proxy_application::SocketCaptureId::from_uuid(Uuid::from_u128(id)),
        runtime_epoch: Uuid::from_u128(100),
        workspace_id,
        listener_id: ListenerId::from_uuid(Uuid::from_u128(200)),
        session_id: Uuid::from_u128(id + 300),
        connection_id: intercept_proxy_application::SocketConnectionId::from_uuid(Uuid::from_u128(
            id + 300,
        )),
        peer_address: "127.0.0.1:43100".to_owned(),
        occurred_at,
        completed_at: occurred_at + Duration::milliseconds(milliseconds),
        payload: SocketCapturePayload::RelayFrame(SocketRelayFrameCapture {
            direction: SocketDirection::Upstream,
            package: package(),
            schema: SocketCaptureSchemaRef {
                id: DocumentSchemaId::new("payment").unwrap(),
                version: 1,
            },
            decode_enabled: false,
            encode_enabled: false,
            origin: vec![0x02, byte, 0x03],
            document: None,
            matched_rule_ids: Vec::new(),
            written: vec![0x02, byte, 0x03],
            write_kind: SocketWriteKind::Original,
            display: SocketDisplayResult::HexFallback {
                reason: SocketDisplayFallbackReason::EncodeDisabled,
                diagnostic: None,
            },
        }),
    }
}

fn query(workspace_id: Option<WorkspaceId>) -> SocketCaptureQuery {
    SocketCaptureQuery {
        workspace_id,
        listener_id: None,
        session_id: None,
        connection_id: None,
        package: None,
        direction: None,
        kind: None,
        occurred_from: None,
        occurred_to: None,
        sort: SocketCaptureSort::OccurredAt,
        direction_sort: SortDirection::Desc,
        page: PageRequest {
            page: 1,
            page_size: 20,
        },
    }
}

#[test]
fn record_query_and_detail_round_trip_exact_application_dto() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let adapter = SocketCaptureRepositoryAdapter::new(store);
    let workspace_id = WorkspaceId::new();
    let first = record(1, workspace_id);
    let second = record(2, workspace_id);
    let returned = adapter.record(first.clone()).expect("record row");
    assert_eq!(returned.capture_id, first.capture_id);
    adapter.record(second.clone()).expect("second record");

    let page = adapter.query(&query(Some(workspace_id))).expect("query");
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.rows[0].capture_id, second.capture_id);
    assert_eq!(page.rows[0].origin_size_bytes, 3);
    assert_eq!(page.rows[0].written_size_bytes, 3);
    assert_eq!(page.rows[0].direction, Some(SocketDirection::Upstream));
    assert_eq!(
        adapter.get_detail(first.capture_id).expect("detail").record,
        first
    );
}

#[test]
fn valid_json_with_mismatched_index_fails_closed() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let adapter = SocketCaptureRepositoryAdapter::new(Arc::clone(&store));
    let original = record(1, WorkspaceId::new());
    let mut insert = insert_from_record(&original).unwrap();
    insert.capture_id = Uuid::from_u128(999);
    store.insert_socket_capture(&insert).unwrap();

    let error = adapter
        .get_detail(intercept_proxy_application::SocketCaptureId::from_uuid(
            insert.capture_id,
        ))
        .expect_err("mismatch");
    assert_eq!(error.view_model.code, "PERSISTENCE_CORRUPT");
}

#[test]
fn semantically_contradictory_payload_fails_on_write_and_restore() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let adapter = SocketCaptureRepositoryAdapter::new(Arc::clone(&store));
    let mut contradictory = record(1, WorkspaceId::new());
    let SocketCapturePayload::RelayFrame(frame) = &mut contradictory.payload else {
        unreachable!();
    };
    frame.decode_enabled = true;
    assert_eq!(
        adapter
            .record(contradictory.clone())
            .unwrap_err()
            .view_model
            .code,
        "SOCKET_CAPTURE_INVALID"
    );

    let mut persisted = record(2, WorkspaceId::new());
    let mut insert = insert_from_record(&persisted).unwrap();
    let SocketCapturePayload::RelayFrame(frame) = &mut persisted.payload else {
        unreachable!();
    };
    frame.decode_enabled = true;
    insert.payload = serde_json::to_value(&persisted).unwrap();
    store.insert_socket_capture(&insert).unwrap();
    assert_eq!(
        adapter
            .get_detail(persisted.capture_id)
            .unwrap_err()
            .view_model
            .code,
        "PERSISTENCE_CORRUPT"
    );

    let mut mismatched_identity = record(3, WorkspaceId::new());
    let mut insert = insert_from_record(&mismatched_identity).unwrap();
    mismatched_identity.session_id = Uuid::from_u128(999);
    insert.session_id = mismatched_identity.session_id;
    insert.payload = serde_json::to_value(&mismatched_identity).unwrap();
    store.insert_socket_capture(&insert).unwrap();
    assert_eq!(
        adapter
            .get_detail(mismatched_identity.capture_id)
            .unwrap_err()
            .view_model
            .code,
        "PERSISTENCE_CORRUPT"
    );
}

#[tokio::test]
async fn socket_records_never_enter_http_capture_query() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let socket = Arc::new(SocketCaptureRepositoryAdapter::new(store));
    let adapter = CaptureRepositoryAdapter::new(
        Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
        socket,
    );
    let workspace_id = WorkspaceId::new();
    adapter
        .record_socket(record(1, workspace_id))
        .expect("socket record");
    adapter
        .record_socket(record(2, WorkspaceId::new()))
        .expect("other workspace socket record");

    let socket_page = CaptureRepositoryPort::query_socket(&adapter, query(Some(workspace_id)))
        .await
        .expect("socket page");
    assert_eq!(socket_page.total, 1);
    let mut connection_query = query(Some(workspace_id));
    connection_query.session_id = Some(Uuid::from_u128(301));
    connection_query.connection_id = Some(
        intercept_proxy_application::SocketConnectionId::from_uuid(Uuid::from_u128(301)),
    );
    assert_eq!(
        CaptureRepositoryPort::query_socket(&adapter, connection_query)
            .await
            .unwrap()
            .total,
        1
    );
    let http_page = CaptureRepositoryPort::query(
        &adapter,
        CaptureQuery {
            keyword: None,
            terminal_ip: None,
            channel: None,
            stage: None,
            result: None,
            rule_id: None,
            after_event_id: None,
            sort: CaptureSort::OccurredAt,
            direction: SortDirection::Desc,
            page: PageRequest {
                page: 1,
                page_size: 20,
            },
        },
    )
    .await
    .expect("HTTP page");
    assert_eq!(http_page.total, 0);
    assert_eq!(
        CaptureRepositoryPort::clear_socket_completed(&adapter, workspace_id)
            .await
            .expect("clear socket"),
        1
    );
    assert_eq!(
        CaptureRepositoryPort::query_socket(&adapter, query(Some(workspace_id)))
            .await
            .unwrap()
            .total,
        0
    );
    assert_eq!(
        CaptureRepositoryPort::query_socket(&adapter, query(None))
            .await
            .unwrap()
            .total,
        1
    );
}

#[test]
fn missing_and_oversized_records_have_stable_errors() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let adapter = SocketCaptureRepositoryAdapter::new(store);
    let missing = adapter
        .get_detail(intercept_proxy_application::SocketCaptureId::new())
        .expect_err("missing");
    assert_eq!(missing.view_model.code, "SOCKET_CAPTURE_NOT_FOUND");

    let mut oversized = record(1, WorkspaceId::new());
    if let SocketCapturePayload::RelayFrame(frame) = &mut oversized.payload {
        frame.origin.resize(65 * 1024 * 1024, 0);
        frame.written.clone_from(&frame.origin);
    }
    let error = adapter.record(oversized).expect_err("oversized");
    assert_eq!(error.view_model.code, "SOCKET_CAPTURE_TOO_LARGE");
}
