use super::*;
use crate::socket_relay::{SocketConnectionTarget, SocketOpenedEvidence};

#[derive(Debug, Default)]
struct RecordingObserver(std::sync::Mutex<Vec<SocketConnectionEvent>>);

impl SocketConnectionObserver for RecordingObserver {
    fn record(&self, event: SocketConnectionEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn lifecycle_rejections_distinguish_cidr_and_capacity() {
    let observer = Arc::new(RecordingObserver::default());
    let events = Arc::new(SocketEventCoordinator::new(observer.clone()));
    let metrics = Arc::new(SocketRelayMetrics::default());
    let run = SocketRelayRunContext {
        listener_id: "listener-1".into(),
        workspace_runtime_epoch: uuid::Uuid::new_v4(),
        listener_run_epoch: uuid::Uuid::new_v4(),
    };
    let lifecycle = SocketLifecycleAdapter {
        events,
        metrics: Arc::clone(&metrics),
        run: Arc::new(std::sync::RwLock::new(run.clone())),
    };
    lifecycle.connection_rejected(
        "192.0.2.1:1234".parse().unwrap(),
        ListenerRejection::NetworkDenied,
    );
    lifecycle.connection_rejected(
        "192.0.2.2:1234".parse().unwrap(),
        ListenerRejection::CapacityExhausted,
    );
    assert_eq!(metrics.snapshot(0).rejected_connections, 2);
    let recorded = observer.0.lock().unwrap();
    assert!(
        matches!(&recorded[0], SocketConnectionEvent::Rejected { run: actual, reason: SocketRejectionReason::Cidr, .. } if actual == &run)
    );
    assert!(matches!(
        &recorded[1],
        SocketConnectionEvent::Rejected {
            reason: SocketRejectionReason::Capacity,
            ..
        }
    ));
}

#[test]
fn panic_and_forced_shutdown_have_typed_terminal_failures() {
    assert_eq!(
        terminal_failure(&TerminalConnectionOutcome::ChildTaskPanicked)
            .unwrap()
            .code,
        "SOCKET_CONNECTION_TASK_PANICKED"
    );
    assert_eq!(
        terminal_failure(&TerminalConnectionOutcome::ShutdownGraceExceeded)
            .unwrap()
            .code,
        LISTENER_SHUTDOWN_GRACE_EXCEEDED
    );
}

#[test]
fn metrics_reset_is_per_run_and_preserves_exact_partial_bytes() {
    let metrics = SocketRelayMetrics::default();
    let id = uuid::Uuid::new_v4();
    let progress = Arc::new(crate::transport::relay::RelayProgress::default());
    progress.add_read(crate::transport::relay::RelayDirection::ClientToServer, 29);
    progress.add_read(crate::transport::relay::RelayDirection::ServerToClient, 13);
    progress.add(crate::transport::relay::RelayDirection::ClientToServer, 19);
    progress.add(crate::transport::relay::RelayDirection::ServerToClient, 7);
    metrics.admitted(id, progress);
    metrics.rejected();
    metrics.opened(id);
    metrics.closed(id, true, RelayBytes::default());
    assert_eq!(
        metrics.snapshot(3),
        SocketRelayMetricsSnapshot {
            active_connections: 0,
            admitted_connections: 1,
            rejected_connections: 1,
            client_to_server_read_bytes: 29,
            client_to_server_bytes: 19,
            server_to_client_read_bytes: 13,
            server_to_client_bytes: 7,
            retained_diagnostic_evictions: 3
        }
    );
    metrics.reset();
    assert_eq!(metrics.snapshot(0), SocketRelayMetricsSnapshot::default());
}

#[test]
fn metrics_snapshot_includes_active_connection_progress_without_double_counting_on_close() {
    let metrics = SocketRelayMetrics::default();
    let id = uuid::Uuid::new_v4();
    let progress = Arc::new(crate::transport::relay::RelayProgress::default());
    progress.add_read(crate::transport::relay::RelayDirection::ClientToServer, 41);
    progress.add_read(crate::transport::relay::RelayDirection::ServerToClient, 17);
    progress.add(crate::transport::relay::RelayDirection::ClientToServer, 31);
    progress.add(crate::transport::relay::RelayDirection::ServerToClient, 11);
    metrics.admitted(id, progress);
    metrics.opened(id);

    let active = metrics.snapshot(0);
    assert_eq!(active.active_connections, 1);
    assert_eq!(active.client_to_server_read_bytes, 41);
    assert_eq!(active.client_to_server_bytes, 31);
    assert_eq!(active.server_to_client_read_bytes, 17);
    assert_eq!(active.server_to_client_bytes, 11);

    metrics.closed(id, true, RelayBytes::default());
    let closed = metrics.snapshot(0);
    assert_eq!(closed.active_connections, 0);
    assert_eq!(closed.client_to_server_read_bytes, 41);
    assert_eq!(closed.client_to_server_bytes, 31);
    assert_eq!(closed.server_to_client_read_bytes, 17);
    assert_eq!(closed.server_to_client_bytes, 11);
}

#[tokio::test]
async fn panic_fallback_emits_the_tracked_partial_byte_counters() {
    let observer = Arc::new(RecordingObserver::default());
    let events = Arc::new(SocketEventCoordinator::new(observer.clone()));
    let metrics = Arc::new(SocketRelayMetrics::default());
    let id = uuid::Uuid::new_v4();
    let epoch = uuid::Uuid::new_v4();
    let run = SocketRelayRunContext {
        listener_id: "listener-1".into(),
        workspace_runtime_epoch: uuid::Uuid::new_v4(),
        listener_run_epoch: epoch,
    };
    let progress = Arc::new(crate::transport::relay::RelayProgress::default());
    progress.add(crate::transport::relay::RelayDirection::ClientToServer, 23);
    progress.add(crate::transport::relay::RelayDirection::ServerToClient, 5);
    metrics.admitted(id, progress);
    metrics.opened(id);
    events.record(SocketConnectionEvent::Admitted {
        run: run.clone(),
        connection_id: id,
        peer: "192.0.2.1:1234".parse().unwrap(),
        target: SocketConnectionTarget::Relay("example.test:443".into()),
        mode: crate::socket_relay::SocketTransportMode::Transparent,
        at: std::time::SystemTime::now(),
    });
    events.record(SocketConnectionEvent::Opened {
        run: run.clone(),
        connection_id: id,
        evidence: SocketOpenedEvidence::Relay {
            resolved_address: "192.0.2.2:443".parse().unwrap(),
            downstream_tls_peer: None,
            upstream_tls: None,
        },
        at: std::time::SystemTime::now(),
    });
    let lifecycle = SocketLifecycleAdapter {
        events,
        metrics,
        run: Arc::new(std::sync::RwLock::new(run)),
    };
    let context = ConnectionContext {
        runtime_epoch: epoch,
        connection_id: id,
        channel: ChannelId::new("listener-1").unwrap(),
        peer_addr: "192.0.2.1:1234".parse().unwrap(),
        accepted_at: std::time::SystemTime::now(),
        tls_peer: None,
    };
    lifecycle.connection_terminal(&context, &TerminalConnectionOutcome::ChildTaskPanicked);
    let recorded = observer.0.lock().unwrap();
    let (bytes, failure) = recorded
        .iter()
        .find_map(|event| match event {
            SocketConnectionEvent::Closed { bytes, failure, .. } => Some((bytes, failure)),
            _ => None,
        })
        .unwrap();
    assert_eq!((bytes.client_to_server, bytes.server_to_client), (23, 5));
    assert_eq!(
        failure.as_ref().map(|value| value.code),
        Some("SOCKET_CONNECTION_TASK_PANICKED")
    );
}
