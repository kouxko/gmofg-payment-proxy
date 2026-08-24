use super::*;

fn rejected(index: u16) -> SocketConnectionEvent {
    SocketConnectionEvent::Rejected {
        run: SocketRelayRunContext {
            workspace_id: "test-workspace".into(),
            listener_id: "listener-1".into(),
            workspace_runtime_epoch: Uuid::nil(),
            listener_run_epoch: Uuid::nil(),
        },
        peer: SocketAddr::from(([127, 0, 0, 1], index)),
        reason: SocketRejectionReason::Capacity,
        code: "SOCKET_CAPACITY_EXHAUSTED",
    }
}

#[test]
fn bounded_retention_evicts_oldest_without_blocking_and_counts_drops() {
    let observer = BoundedSocketConnectionObserver::new(2).unwrap();
    observer.record(rejected(1));
    observer.record(rejected(2));
    observer.record(rejected(3));

    assert_eq!(observer.snapshot(), vec![rejected(2), rejected(3)]);
    assert_eq!(observer.retained_diagnostic_evictions(), 1);

    observer.begin_run();
    assert!(observer.snapshot().is_empty());
    assert_eq!(observer.retained_diagnostic_evictions(), 0);
}

#[test]
fn bounded_retention_rejects_zero_limits() {
    assert!(BoundedSocketConnectionObserver::with_limits(0, 1).is_err());
    assert!(BoundedSocketConnectionObserver::with_limits(1, 0).is_err());
}
