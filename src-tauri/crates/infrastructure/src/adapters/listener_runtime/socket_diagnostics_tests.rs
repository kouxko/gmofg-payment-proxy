use std::{net::SocketAddr, time::SystemTime};

use intercept_proxy_application::UiEventPayload;
use intercept_proxy_runtime::{
    SocketConnectionTarget, SocketOpenedEvidence, SocketRelayBytes, SocketRelayRunContext,
    SocketRelayStage, SocketTransportMode,
};
use uuid::Uuid;

use super::*;

fn run() -> SocketRelayRunContext {
    SocketRelayRunContext {
        listener_id: "listener-socket-1".into(),
        workspace_runtime_epoch: Uuid::new_v4(),
        listener_run_epoch: Uuid::new_v4(),
    }
}

fn record_partial_relay_failure(
    observer: &SocketDiagnosticObserver,
    run: &SocketRelayRunContext,
    connection_id: Uuid,
    at: SystemTime,
) {
    observer.record(SocketConnectionEvent::Admitted {
        run: run.clone(),
        connection_id,
        peer: "192.0.2.10:1234".parse().unwrap(),
        target: SocketConnectionTarget::Relay("example.test:443".into()),
        mode: SocketTransportMode::TcpToTls,
        at,
    });
    observer.record(SocketConnectionEvent::Opened {
        run: run.clone(),
        connection_id,
        evidence: SocketOpenedEvidence::Relay {
            resolved_address: "192.0.2.20:443".parse().unwrap(),
            downstream_tls_peer: None,
            upstream_tls: None,
        },
        at,
    });
    observer.record(SocketConnectionEvent::Closed {
        run: run.clone(),
        connection_id,
        target: SocketConnectionTarget::Relay("example.test:443".into()),
        opened: true,
        bytes: SocketRelayBytes {
            client_to_server_read: 43,
            client_to_server: 37,
            server_to_client_read: 17,
            server_to_client: 11,
        },
        failure: Some(SocketRelayFailure {
            stage: SocketRelayStage::RelayWrite,
            direction: Some(SocketRelayDirection::ClientToServer),
            code: "SOCKET_WRITE_FAILED",
        }),
        at,
    });
}

#[test]
fn typed_sequence_keeps_order_epochs_direction_and_partial_bytes() {
    let events = Arc::new(EventHub::new(16));
    let observer = SocketDiagnosticObserver::with_capacity(Arc::clone(&events), 16);
    let run = run();
    let connection_id = Uuid::new_v4();
    let at = SystemTime::now();
    record_partial_relay_failure(&observer, &run, connection_id, at);

    let replay = events.replay_after(0);
    assert_eq!(replay.events.len(), 3);
    assert!(
        replay
            .events
            .iter()
            .all(|event| event.runtime_epoch == Some(run.workspace_runtime_epoch))
    );
    assert_eq!(
        replay.events[0].entity_id.as_deref(),
        Some(connection_id.to_string().as_str())
    );
    let summaries = replay
        .events
        .iter()
        .map(|event| match &event.payload {
            UiEventPayload::DiagnosticLogAdded(entry) => entry.summary.as_str(),
            _ => panic!("socket observer emitted a non-diagnostic event"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summaries,
        [
            "Socket 连接已接纳",
            "Socket 上游连接已建立",
            "Socket 连接已失败"
        ]
    );
    let UiEventPayload::DiagnosticLogAdded(closed) = &replay.events[2].payload else {
        unreachable!()
    };
    assert_eq!(closed.stage, DiagnosticLogStage::Socket);
    let context = closed
        .socket_context
        .as_ref()
        .expect("typed socket context");
    assert_eq!(
        context.connection_id.as_deref(),
        Some(connection_id.to_string().as_str())
    );
    assert_eq!(
        context.workspace_runtime_epoch,
        run.workspace_runtime_epoch.to_string()
    );
    assert_eq!(
        context.listener_run_epoch,
        run.listener_run_epoch.to_string()
    );
    assert_eq!(context.stage, SocketDiagnosticStage::RelayWrite);
    assert_eq!(
        context.direction,
        Some(SocketDiagnosticDirection::ClientToServer)
    );
    assert_eq!(context.client_to_server_read_bytes, 43);
    assert_eq!(context.client_to_server_bytes, 37);
    assert_eq!(context.server_to_client_read_bytes, 17);
    assert_eq!(context.server_to_client_bytes, 11);
    let detail = closed.detail.as_deref().unwrap();
    assert!(detail.contains("客户端读取：43 字节"), "{detail}");
    assert!(detail.contains("客户端→上游：37 字节"), "{detail}");
    assert!(detail.contains("上游读取：17 字节"), "{detail}");
    assert!(detail.contains("上游→客户端：11 字节"), "{detail}");
    assert!(detail.contains("方向：客户端→上游"), "{detail}");
}

#[test]
fn local_responder_diagnostics_never_invent_upstream_evidence() {
    let events = Arc::new(EventHub::new(16));
    let observer = SocketDiagnosticObserver::with_capacity(Arc::clone(&events), 16);
    let run = run();
    let connection_id = Uuid::new_v4();
    let at = SystemTime::now();

    observer.record(SocketConnectionEvent::Admitted {
        run: run.clone(),
        connection_id,
        peer: "192.0.2.10:1234".parse().unwrap(),
        target: SocketConnectionTarget::LocalResponder,
        mode: SocketTransportMode::Transparent,
        at,
    });
    observer.record(SocketConnectionEvent::Opened {
        run: run.clone(),
        connection_id,
        evidence: SocketOpenedEvidence::LocalResponder {
            downstream_tls_peer: None,
        },
        at,
    });
    observer.record(SocketConnectionEvent::Closed {
        run,
        connection_id,
        target: SocketConnectionTarget::LocalResponder,
        opened: true,
        bytes: SocketRelayBytes {
            client_to_server_read: 7,
            client_to_server: 0,
            server_to_client_read: 0,
            server_to_client: 11,
        },
        failure: None,
        at,
    });

    let replay = events.replay_after(0);
    assert_eq!(replay.events.len(), 3);
    let entries = replay
        .events
        .iter()
        .map(|event| match &event.payload {
            UiEventPayload::DiagnosticLogAdded(entry) => entry,
            _ => panic!("socket observer emitted a non-diagnostic event"),
        })
        .collect::<Vec<_>>();
    assert_eq!(entries[1].summary, "Socket 本地应答已就绪");
    assert_eq!(
        entries[1].socket_context.as_ref().unwrap().stage,
        SocketDiagnosticStage::Admission
    );
    let closed = entries[2].socket_context.as_ref().unwrap();
    assert_eq!(closed.client_to_server_read_bytes, 7);
    assert_eq!(closed.client_to_server_bytes, 0);
    assert_eq!(closed.server_to_client_read_bytes, 0);
    assert_eq!(closed.server_to_client_bytes, 11);
    for entry in entries {
        let detail = entry.detail.as_deref().unwrap();
        assert!(detail.contains("本地应答（无上游）"), "{detail}");
        assert!(!detail.contains("目标："), "{detail}");
        assert!(!detail.contains("上游："), "{detail}");
        assert!(!detail.contains("上游 TLS"), "{detail}");
        assert!(!detail.contains("Connect"), "{detail}");
    }
}

#[test]
fn local_responder_tls_ready_uses_downstream_tls_stage_only() {
    let run = run();
    let connection_id = Uuid::new_v4();
    let entry = opened_entry(
        &run,
        connection_id,
        &SocketOpenedEvidence::LocalResponder {
            downstream_tls_peer: Some("sha256:test-client".into()),
        },
    );

    assert_eq!(entry.summary, "Socket 本地应答已就绪");
    assert_eq!(
        entry.socket_context.unwrap().stage,
        SocketDiagnosticStage::DownstreamTls
    );
    let detail = entry.detail.unwrap();
    assert!(
        detail.contains("App 侧 TLS：sha256:test-client"),
        "{detail}"
    );
    assert!(!detail.contains("上游："), "{detail}");
    assert!(!detail.contains("上游 TLS"), "{detail}");
}

#[test]
fn bounded_retention_drop_count_is_observable_and_resets_per_run() {
    let events = Arc::new(EventHub::new(16));
    let observer = SocketDiagnosticObserver::with_capacity(events, 2);
    let run = run();
    for port in 1..=3 {
        observer.record(SocketConnectionEvent::Rejected {
            run: run.clone(),
            peer: SocketAddr::from(([192, 0, 2, 1], port)),
            reason: intercept_proxy_runtime::SocketRejectionReason::Capacity,
            code: "SOCKET_CAPACITY_EXHAUSTED",
        });
    }
    assert_eq!(observer.retained_diagnostic_evictions(), 1);
    observer.begin_run();
    assert_eq!(observer.retained_diagnostic_evictions(), 0);
}

#[test]
fn tls_failures_map_to_the_correct_application_stage() {
    for (stage, expected) in [
        (
            SocketRelayStage::DownstreamTls,
            DiagnosticLogStage::DownstreamTls,
        ),
        (
            SocketRelayStage::UpstreamTls,
            DiagnosticLogStage::UpstreamTls,
        ),
    ] {
        assert_eq!(
            diagnostic_stage(SocketRelayFailure {
                stage,
                direction: None,
                code: "TEST",
            }),
            expected
        );
    }
}

#[test]
fn frame_processing_and_local_exchange_keep_typed_diagnostic_values() {
    assert_eq!(
        application_stage(SocketRelayStage::FrameInspect),
        SocketDiagnosticStage::FrameInspect
    );
    assert_eq!(
        application_stage(SocketRelayStage::FrameProcess),
        SocketDiagnosticStage::FrameProcess
    );
    assert_eq!(
        application_direction(SocketRelayDirection::LocalExchange),
        SocketDiagnosticDirection::LocalExchange
    );
}

#[test]
fn concurrent_connections_and_restarted_runs_keep_distinct_typed_identity() {
    let events = Arc::new(EventHub::new(16));
    let observer = Arc::new(SocketDiagnosticObserver::with_capacity(
        Arc::clone(&events),
        16,
    ));
    let first_run = run();
    let mut second_run = run();
    second_run.listener_id = first_run.listener_id.clone();
    let first_connection = Uuid::new_v4();
    let second_connection = Uuid::new_v4();
    let record = |observer: Arc<SocketDiagnosticObserver>, run, connection_id, bytes| {
        std::thread::spawn(move || {
            observer.record(SocketConnectionEvent::Closed {
                run,
                connection_id,
                target: SocketConnectionTarget::Relay("example.test:443".into()),
                opened: true,
                bytes,
                failure: None,
                at: SystemTime::now(),
            });
        })
    };
    let first = record(
        Arc::clone(&observer),
        first_run.clone(),
        first_connection,
        SocketRelayBytes {
            client_to_server_read: 5,
            client_to_server: 1,
            server_to_client_read: 6,
            server_to_client: 2,
        },
    );
    observer.begin_run();
    let second = record(
        Arc::clone(&observer),
        second_run.clone(),
        second_connection,
        SocketRelayBytes {
            client_to_server_read: 7,
            client_to_server: 3,
            server_to_client_read: 8,
            server_to_client: 4,
        },
    );
    first.join().unwrap();
    second.join().unwrap();

    let replay = events.replay_after(0);
    assert_eq!(replay.events.len(), 2);
    let mut identities = replay
        .events
        .iter()
        .map(|event| {
            let UiEventPayload::DiagnosticLogAdded(entry) = &event.payload else {
                unreachable!()
            };
            let context = entry.socket_context.as_ref().unwrap();
            (
                event.entity_id.clone().unwrap(),
                context.listener_run_epoch.clone(),
                context.client_to_server_bytes,
            )
        })
        .collect::<Vec<_>>();
    identities.sort();
    assert!(identities.contains(&(
        first_connection.to_string(),
        first_run.listener_run_epoch.to_string(),
        1,
    )));
    assert!(identities.contains(&(
        second_connection.to_string(),
        second_run.listener_run_epoch.to_string(),
        3,
    )));
}
