use super::*;
use crate::{SqliteStore, adapters::SocketCaptureRepositoryAdapter};
use chrono::{Duration, TimeZone, Utc};
use intercept_proxy_application::{
    EventHub, SocketCaptureId, SocketCapturePayload, SocketCaptureRecord, SocketCaptureSchemaRef,
    SocketConnectionId, SocketDisplayFallbackReason, SocketDisplayResult, SocketRelayFrameCapture,
    SocketWriteKind,
};
use intercept_proxy_domain::{
    DocumentSchemaId, ListenerId, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    SocketDirection, WorkspaceId,
};
use parking_lot::RwLock;
use uuid::Uuid;

fn record(id: u128, workspace_id: WorkspaceId) -> SocketCaptureRecord {
    let occurred_at = Utc.with_ymd_and_hms(2026, 8, 15, 10, 0, 0).unwrap();
    SocketCaptureRecord {
        capture_id: SocketCaptureId::from_uuid(Uuid::from_u128(id)),
        runtime_epoch: Uuid::from_u128(10),
        workspace_id,
        listener_id: ListenerId::from_uuid(Uuid::from_u128(11)),
        session_id: Uuid::from_u128(12),
        connection_id: SocketConnectionId::from_uuid(Uuid::from_u128(12)),
        peer_address: "127.0.0.1:43100".to_owned(),
        occurred_at,
        completed_at: occurred_at + Duration::milliseconds(7),
        payload: SocketCapturePayload::RelayFrame(SocketRelayFrameCapture {
            direction: SocketDirection::Upstream,
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("iso8583").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            schema: SocketCaptureSchemaRef {
                id: DocumentSchemaId::new("payment").unwrap(),
                version: 1,
            },
            decode_enabled: false,
            encode_enabled: false,
            origin: vec![0x02, 0x03],
            document: None,
            matched_rule_ids: Vec::new(),
            written: vec![0x02, 0x03],
            write_kind: SocketWriteKind::Original,
            display: SocketDisplayResult::HexFallback {
                reason: SocketDisplayFallbackReason::EncodeDisabled,
                diagnostic: None,
            },
        }),
    }
}

#[test]
fn logical_byte_budget_releases_on_success_full_disconnect_and_drop() {
    let budget = Arc::new(QueueBudget {
        used: AtomicU64::new(0),
        limit: 10,
    });
    let first = budget.reserve(6).unwrap();
    assert!(budget.reserve(5).is_none());
    let (sender, receiver) = sync_channel(1);
    sender.try_send(first).unwrap();
    assert_eq!(budget.used.load(Ordering::Acquire), 6);

    let full = budget.reserve(4).unwrap();
    assert!(matches!(sender.try_send(full), Err(TrySendError::Full(_))));
    assert_eq!(budget.used.load(Ordering::Acquire), 6);
    drop(receiver.recv().unwrap());
    assert_eq!(budget.used.load(Ordering::Acquire), 0);

    drop(receiver);
    let disconnected = budget.reserve(10).unwrap();
    assert!(matches!(
        sender.try_send(disconnected),
        Err(TrySendError::Disconnected(_))
    ));
    assert_eq!(budget.used.load(Ordering::Acquire), 0);
    assert_eq!(SOCKET_CAPTURE_QUEUE_CAPACITY, 256);
    assert_eq!(SOCKET_CAPTURE_QUEUE_MAX_LOGICAL_BYTES, 64 * 1024 * 1024);
}

#[test]
fn clean_shutdown_drains_and_joins_committed_captures() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(Arc::clone(&store)));
    let publisher = SocketCapturePublisher::new(
        Arc::clone(&repository),
        Arc::new(RwLock::new(Arc::new(EventHub::default()))),
    );
    let workspace_id = WorkspaceId::new();
    let committed = record(1, workspace_id);

    let ticket = publisher.ticket(workspace_id);
    publisher.publish(committed.clone(), ticket);
    assert!(publisher.close_and_drain());
    assert_eq!(
        repository
            .get_detail(committed.capture_id)
            .unwrap()
            .record
            .completed_at,
        committed.completed_at
    );
    assert_eq!(publisher.inner.budget.used.load(Ordering::Acquire), 0);
}

#[test]
fn publishing_while_drain_holds_mutation_gate_keeps_every_committed_capture() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(Arc::clone(&store)));
    let publisher = SocketCapturePublisher::new(
        Arc::clone(&repository),
        Arc::new(RwLock::new(Arc::new(EventHub::default()))),
    );
    let workspace_id = WorkspaceId::new();
    let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let blocking_store = Arc::clone(&store);
    let blocker = thread::spawn(move || {
        blocking_store.block_socket_capture_mutation_for_test(&entered_sender, &release_receiver);
    });
    entered_receiver.recv().unwrap();

    let committed = (1..=4)
        .map(|id| record(id, workspace_id))
        .collect::<Vec<_>>();
    for capture in &committed {
        let ticket = publisher.ticket(workspace_id);
        publisher.publish(capture.clone(), ticket);
    }
    release_sender.send(()).unwrap();
    blocker.join().unwrap();
    assert!(publisher.close_and_drain());

    for capture in committed {
        assert_eq!(
            repository
                .get_detail(capture.capture_id)
                .unwrap()
                .record
                .capture_id,
            capture.capture_id
        );
    }
}

#[test]
fn shutdown_timeout_detaches_once_and_never_rejoins_on_drop() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(store));
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let _ = release_receiver.recv();
    });
    let (_done_sender, done_receiver) = std::sync::mpsc::channel();
    let inner = PublisherInner {
        sender: Mutex::new(None),
        worker: Mutex::new(Some(worker)),
        worker_done: Mutex::new(Some(done_receiver)),
        repository,
        events: Arc::new(RwLock::new(Arc::new(EventHub::default()))),
        budget: Arc::new(QueueBudget {
            used: AtomicU64::new(0),
            limit: 10,
        }),
        queue_full_warned: Arc::new(AtomicBool::new(false)),
        disconnected_warned: Arc::new(AtomicBool::new(false)),
        display_gate: Mutex::new(None),
        completion_event_gate: Arc::new(Mutex::new(None)),
    };

    assert!(!inner.close_and_drain(std::time::Duration::ZERO));
    assert!(inner.worker.lock().is_none());
    release_sender.send(()).unwrap();
    drop(inner);
}

#[test]
fn real_publisher_accepts_worker_plus_256_queued_captures_and_drops_258th() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(Arc::clone(&store)));
    let events = Arc::new(RwLock::new(Arc::new(EventHub::default())));
    let publisher = SocketCapturePublisher::new(Arc::clone(&repository), Arc::clone(&events));
    let workspace_id = WorkspaceId::new();
    let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let blocking_store = Arc::clone(&store);
    let blocker = thread::spawn(move || {
        blocking_store.block_socket_capture_mutation_for_test(&entered_sender, &release_receiver);
    });
    entered_receiver.recv().unwrap();

    let one_capture_bytes = record(1, workspace_id).logical_bytes();
    for id in 1..=256 {
        publisher.publish(record(id, workspace_id), publisher.ticket(workspace_id));
    }
    let first_256_bytes = one_capture_bytes * 256;
    assert_eq!(
        publisher.inner.budget.used.load(Ordering::Acquire),
        first_256_bytes
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while publisher.inner.budget.used.load(Ordering::Acquire) == first_256_bytes {
        publisher.publish(record(257, workspace_id), publisher.ticket(workspace_id));
        assert!(
            std::time::Instant::now() < deadline,
            "drain worker did not receive the in-flight capture"
        );
        thread::yield_now();
    }
    assert_eq!(
        publisher.inner.budget.used.load(Ordering::Acquire),
        one_capture_bytes * 257
    );
    publisher.publish(record(258, workspace_id), publisher.ticket(workspace_id));
    assert_eq!(
        publisher.inner.budget.used.load(Ordering::Acquire),
        one_capture_bytes * 257
    );

    release_sender.send(()).unwrap();
    blocker.join().unwrap();
    assert!(publisher.close_and_drain());
    assert_eq!(
        repository
            .query(&capture_query(workspace_id))
            .unwrap()
            .total,
        257
    );
    assert_eq!(
        repository
            .get_detail(SocketCaptureId::from_uuid(Uuid::from_u128(257)))
            .unwrap()
            .record
            .capture_id,
        SocketCaptureId::from_uuid(Uuid::from_u128(257))
    );
    assert!(
        repository
            .get_detail(SocketCaptureId::from_uuid(Uuid::from_u128(258)))
            .is_err()
    );
    assert_eq!(resource_warning_count(&events), 1);
}

#[test]
fn real_publisher_counts_worker_in_flight_bytes_against_64_mib_budget() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(Arc::clone(&store)));
    let events = Arc::new(RwLock::new(Arc::new(EventHub::default())));
    let publisher = SocketCapturePublisher::new(Arc::clone(&repository), Arc::clone(&events));
    let workspace_id = WorkspaceId::new();
    let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let blocking_store = Arc::clone(&store);
    let blocker = thread::spawn(move || {
        blocking_store.block_socket_capture_mutation_for_test(&entered_sender, &release_receiver);
    });
    entered_receiver.recv().unwrap();

    let boundary =
        record_with_logical_bytes(1, workspace_id, SOCKET_CAPTURE_QUEUE_MAX_LOGICAL_BYTES);
    publisher.publish(boundary, publisher.ticket(workspace_id));
    assert_eq!(
        publisher.inner.budget.used.load(Ordering::Acquire),
        SOCKET_CAPTURE_QUEUE_MAX_LOGICAL_BYTES
    );
    publisher.publish(record(2, workspace_id), publisher.ticket(workspace_id));
    assert_eq!(
        publisher.inner.budget.used.load(Ordering::Acquire),
        SOCKET_CAPTURE_QUEUE_MAX_LOGICAL_BYTES
    );

    release_sender.send(()).unwrap();
    blocker.join().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while publisher.inner.budget.used.load(Ordering::Acquire) != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "64 MiB capture reservation was not released"
        );
        thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(publisher.close_and_drain());
    assert_eq!(publisher.inner.budget.used.load(Ordering::Acquire), 0);
    assert!(
        repository
            .get_detail(SocketCaptureId::from_uuid(Uuid::from_u128(1)))
            .is_ok()
    );
    assert!(
        repository
            .get_detail(SocketCaptureId::from_uuid(Uuid::from_u128(2)))
            .is_err()
    );
    assert_eq!(resource_warning_count(&events), 1);
}

#[path = "tests/completion_events.rs"]
mod completion_events;

#[test]
fn display_capture_maps_every_fallback_without_dynamic_script_text() {
    for (input, expected_reason, expected_code) in [
        (
            ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EncodeDisabled),
            SocketDisplayFallbackReason::EncodeDisabled,
            None,
        ),
        (
            ProtocolDisplayResult::HexFallback(DisplayFallbackReason::NotDeclared),
            SocketDisplayFallbackReason::NotDeclared,
            None,
        ),
        (
            ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed),
            SocketDisplayFallbackReason::EntryPointFailed,
            Some("DISPLAY_ENTRY_FAILED"),
        ),
        (
            ProtocolDisplayResult::HexFallback(DisplayFallbackReason::ResourceLimitExceeded(
                intercept_proxy_protocol_scripting::ProtocolResourceLimit::Operations,
            )),
            SocketDisplayFallbackReason::ResourceLimitExceeded,
            Some("DISPLAY_RESOURCE_LIMIT_EXCEEDED"),
        ),
    ] {
        let SocketDisplayResult::HexFallback { reason, diagnostic } = capture_display(input) else {
            panic!("expected hex fallback")
        };
        assert_eq!(reason, expected_reason);
        assert_eq!(
            diagnostic.as_ref().map(|value| value.code.as_str()),
            expected_code
        );
    }
    assert_eq!(
        capture_display(ProtocolDisplayResult::UntrustedHtml("<p>ok</p>".into())),
        SocketDisplayResult::UntrustedHtml {
            html: "<p>ok</p>".into()
        }
    );
    let SocketDisplayResult::HexFallback {
        diagnostic: Some(diagnostic),
        ..
    } = capture_resource_busy()
    else {
        panic!("busy display must fall back")
    };
    assert_eq!(diagnostic.code, "DISPLAY_RESOURCE_BUSY");
}

#[test]
fn stale_generation_ticket_never_persists_a_completed_capture() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(Arc::clone(&store)));
    let publisher = SocketCapturePublisher::new(
        Arc::clone(&repository),
        Arc::new(RwLock::new(Arc::new(EventHub::default()))),
    );
    let workspace_id = WorkspaceId::new();
    let ticket = publisher.ticket(workspace_id);
    repository.clear_completed(workspace_id).unwrap();

    publisher.publish(record(1, workspace_id), ticket);

    assert!(publisher.close_and_drain());
    assert_eq!(
        repository
            .query(&capture_query(workspace_id))
            .unwrap()
            .total,
        0
    );
}

#[test]
fn closed_real_publisher_drops_capture_and_warns_once() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(store));
    let events = Arc::new(RwLock::new(Arc::new(EventHub::default())));
    let publisher = SocketCapturePublisher::new(Arc::clone(&repository), Arc::clone(&events));
    let workspace_id = WorkspaceId::new();
    let ticket = publisher.ticket(workspace_id);
    assert!(publisher.close_and_drain());

    publisher.publish(record(1, workspace_id), ticket);

    assert_eq!(
        repository
            .query(&capture_query(workspace_id))
            .unwrap()
            .total,
        0
    );
    assert_eq!(resource_warning_count(&events), 1);
    let debug = format!("{publisher:?}");
    assert!(debug.contains("queue_capacity: 256"));
    assert!(debug.contains("queue_logical_bytes: 67108864"));
}

fn capture_query(workspace_id: WorkspaceId) -> intercept_proxy_application::SocketCaptureQuery {
    intercept_proxy_application::SocketCaptureQuery {
        workspace_id: Some(workspace_id),
        listener_id: None,
        session_id: None,
        connection_id: None,
        package: None,
        direction: None,
        kind: None,
        occurred_from: None,
        occurred_to: None,
        sort: intercept_proxy_application::SocketCaptureSort::OccurredAt,
        direction_sort: intercept_proxy_application::SortDirection::Asc,
        page: intercept_proxy_application::PageRequest {
            page: 1,
            page_size: 500,
        },
    }
}

fn resource_warning_count(events: &RwLock<Arc<EventHub>>) -> usize {
    events
        .read()
        .replay_after(0)
        .events
        .into_iter()
        .filter(|event| matches!(event.payload, UiEventPayload::ResourceWarning { .. }))
        .count()
}

fn record_with_logical_bytes(
    id: u128,
    workspace_id: WorkspaceId,
    target: u64,
) -> SocketCaptureRecord {
    let mut value = record(id, workspace_id);
    if let SocketCapturePayload::RelayFrame(frame) = &mut value.payload {
        frame.origin = vec![0];
        frame.written = vec![0];
    }
    let remaining = target.checked_sub(value.logical_bytes()).unwrap();
    let paired = usize::try_from(remaining / 2).unwrap();
    if let SocketCapturePayload::RelayFrame(frame) = &mut value.payload {
        frame.origin.resize(1 + paired, 0);
        frame.written.resize(1 + paired, 0);
    }
    if remaining % 2 == 1 {
        value.peer_address.push('x');
    }
    assert_eq!(value.logical_bytes(), target);
    value
}
