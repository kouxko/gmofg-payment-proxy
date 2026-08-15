//! 插入完成到 completion event 发布之间的 clear/reset 线性化回归。

use super::*;

fn completed_capture_ids(events: &EventHub) -> Vec<SocketCaptureId> {
    events
        .replay_after(0)
        .events
        .into_iter()
        .filter_map(|event| match event.payload {
            UiEventPayload::SocketCaptureCompleted(row) => Some(row.capture_id),
            _ => None,
        })
        .collect()
}

fn assert_no_late_event_after_clear(reset_all: bool, capture_number: u128) {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(Arc::clone(&store)));
    let event_hub = Arc::new(EventHub::default());
    let publisher = SocketCapturePublisher::new(
        Arc::clone(&repository),
        Arc::new(RwLock::new(Arc::clone(&event_hub))),
    );
    let workspace_id = WorkspaceId::new();
    let committed = record(capture_number, workspace_id);
    let (entered_sender, entered_receiver) = sync_channel(1);
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    publisher.block_next_completion_event(entered_sender, release_receiver);
    publisher.publish(committed.clone(), publisher.ticket(workspace_id));
    entered_receiver.recv().unwrap();

    let (cleared_sender, cleared_receiver) = sync_channel(1);
    let clear_repository = Arc::clone(&repository);
    let clear_store = Arc::clone(&store);
    let clear = thread::spawn(move || {
        let deleted = if reset_all {
            usize::try_from(clear_store.clear_socket_captures(None).unwrap()).unwrap()
        } else {
            clear_repository.clear_completed(workspace_id).unwrap()
        };
        cleared_sender.send(deleted).unwrap();
    });
    assert!(
        cleared_receiver
            .recv_timeout(std::time::Duration::from_millis(20))
            .is_err()
    );
    release_sender.send(()).unwrap();
    assert_eq!(cleared_receiver.recv().unwrap(), 1);
    clear.join().unwrap();
    assert_eq!(completed_capture_ids(&event_hub), [committed.capture_id]);

    assert!(publisher.close_and_drain());
    assert_eq!(completed_capture_ids(&event_hub), [committed.capture_id]);
    assert!(repository.get_detail(committed.capture_id).is_err());
}

#[test]
fn workspace_clear_waits_for_inserted_completion_event_and_none_arrive_after_return() {
    assert_no_late_event_after_clear(false, 24);
}

#[test]
fn global_reset_waits_for_inserted_completion_event_and_none_arrive_after_return() {
    assert_no_late_event_after_clear(true, 25);
}

#[test]
fn early_ticket_is_invalidated_only_by_its_workspace_clear() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(Arc::clone(&store)));
    let publisher = SocketCapturePublisher::new(
        Arc::clone(&repository),
        Arc::new(RwLock::new(Arc::new(EventHub::default()))),
    );
    let workspace_a = WorkspaceId::new();
    let workspace_b = WorkspaceId::new();
    let stale_a = record(21, workspace_a);
    let current_b = record(22, workspace_b);

    let ticket_a = publisher.ticket(workspace_a);
    let ticket_b = publisher.ticket(workspace_b);
    assert_eq!(repository.clear_completed(workspace_a).unwrap(), 0);
    publisher.publish(stale_a.clone(), ticket_a);
    publisher.publish(current_b.clone(), ticket_b);
    assert!(publisher.close_and_drain());

    assert!(repository.get_detail(stale_a.capture_id).is_err());
    assert_eq!(
        repository
            .get_detail(current_b.capture_id)
            .unwrap()
            .record
            .capture_id,
        current_b.capture_id
    );
}

#[test]
fn early_ticket_is_invalidated_by_global_capture_reset() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(SocketCaptureRepositoryAdapter::new(Arc::clone(&store)));
    let publisher = SocketCapturePublisher::new(
        Arc::clone(&repository),
        Arc::new(RwLock::new(Arc::new(EventHub::default()))),
    );
    let workspace_id = WorkspaceId::new();
    let stale = record(23, workspace_id);
    let ticket = publisher.ticket(workspace_id);

    assert_eq!(store.clear_socket_captures(None).unwrap(), 0);
    publisher.publish(stale.clone(), ticket);
    assert!(publisher.close_and_drain());
    assert!(repository.get_detail(stale.capture_id).is_err());
}
