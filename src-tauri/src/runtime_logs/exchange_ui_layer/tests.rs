use std::{sync::Arc, time::Duration};

use intercept_proxy_application::{
    CapacityLedger, EventHub, ExchangeContext, ExchangeObservationEvent, UiEventPayload,
};
use intercept_proxy_infrastructure::ExchangeObservationStore;
use tracing::subscriber::with_default;
use tracing_subscriber::prelude::*;

use super::{ExchangeUiConsumer, ExchangeUiLayer};
use crate::runtime_logs::nonblocking_queue::QueueByteBudget;

#[path = "tests/process.rs"]
mod process;
#[path = "tests/queue.rs"]
mod queue;

fn queue_budget() -> Arc<QueueByteBudget> {
    Arc::new(QueueByteBudget::new(64 * 1024))
}

fn layer(store: &Arc<ExchangeObservationStore>) -> (ExchangeUiLayer, ExchangeUiConsumer) {
    ExchangeUiLayer::new(
        Arc::clone(store),
        Arc::new(EventHub::new(64)),
        64,
        queue_budget(),
    )
    .expect("observation consumer thread")
}

#[test]
fn successful_observation_write_publishes_a_realtime_exchange_change_event() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let events = Arc::new(EventHub::new(64));
    let (layer, consumer) =
        ExchangeUiLayer::new(Arc::clone(&store), Arc::clone(&events), 64, queue_budget())
            .expect("observation consumer thread");
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = "ex-live",
            workspace_id = "10000000-0000-0000-0000-000000000001",
            listener_id = "20000000-0000-0000-0000-000000000002",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:9000",
            protocol = "socket"
        );
        let _entered = span.enter();
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
    });
    consumer.shutdown().unwrap();

    let replay = events.replay_after(0);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].entity_id.as_deref(), Some("ex-live"));
    assert!(matches!(
        replay.events[0].payload,
        UiEventPayload::ExchangeObservationChanged
    ));
}

#[test]
fn accepted_append_reuses_opened_runtime_epoch_and_publishes_realtime_change() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let events = Arc::new(EventHub::new(64));
    let (layer, consumer) =
        ExchangeUiLayer::new(Arc::clone(&store), Arc::clone(&events), 64, queue_budget())
            .expect("observation consumer thread");
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = "ex-append-live",
            workspace_id = "10000000-0000-0000-0000-000000000001",
            listener_id = "20000000-0000-0000-0000-000000000002",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:9000",
            protocol = "socket"
        );
        {
            let _entered = span.enter();
            tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
        }

        // A later producer only needs the stable Exchange identity. Publication must use the
        // accepted record's metadata instead of requiring every event to repeat runtime_epoch.
        tracing::info!(
            target: "intercept_proxy::exchange::ui",
            event = "received",
            exchange_id = "ex-append-live",
            protocol = "socket",
            direction = "upstream",
            context_bytes_hex = "01"
        );
    });
    consumer.shutdown().unwrap();

    assert_eq!(store.get("ex-append-live").unwrap().events.len(), 2);
    let replay = events.replay_after(0);
    assert_eq!(replay.events.len(), 2);
    assert_eq!(
        replay.events[1].runtime_epoch,
        Some("30000000-0000-0000-0000-000000000003".parse().unwrap())
    );
    assert_eq!(
        replay.events[1].entity_id.as_deref(),
        Some("ex-append-live")
    );
    assert!(matches!(
        replay.events[1].payload,
        UiEventPayload::ExchangeObservationChanged
    ));
}

#[test]
fn append_rejects_protocol_identity_that_disagrees_with_opened_record() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let events = Arc::new(EventHub::new(64));
    let (layer, consumer) =
        ExchangeUiLayer::new(Arc::clone(&store), Arc::clone(&events), 64, queue_budget())
            .expect("observation consumer thread");
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = "ex-protocol",
            workspace_id = "10000000-0000-0000-0000-000000000001",
            listener_id = "20000000-0000-0000-0000-000000000002",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:9000",
            protocol = "http"
        );
        {
            let _entered = span.enter();
            tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
        }
        tracing::info!(
            target: "intercept_proxy::exchange::ui",
            event = "received",
            exchange_id = "ex-protocol",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            protocol = "socket",
            direction = "upstream",
            context_bytes_hex = "01"
        );
    });
    consumer.shutdown().unwrap();

    assert_eq!(store.get("ex-protocol").unwrap().events.len(), 1);
    assert_eq!(store.ignored_events(), 1);
    let replay = events.replay_after(0);
    assert_eq!(replay.events.len(), 2);
    assert_eq!(replay.events[1].entity_id.as_deref(), Some("ex-protocol"));
}

#[test]
fn every_accepted_append_kind_publishes_the_same_exchange_entity() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let events = Arc::new(EventHub::new(64));
    let (layer, consumer) =
        ExchangeUiLayer::new(Arc::clone(&store), Arc::clone(&events), 64, queue_budget()).unwrap();
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = "ex-kinds",
            workspace_id = "10000000-0000-0000-0000-000000000001",
            listener_id = "20000000-0000-0000-0000-000000000002",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:9000",
            protocol = "socket"
        );
        let _entered = span.enter();
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "sent", direction = "upstream", context_bytes_hex = "01");
        tracing::error!(target: "intercept_proxy::exchange::ui", event = "failed", direction = "downstream", stage = "read", error = "peer closed");
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "closed", outcome = "failed");
    });
    consumer.shutdown().unwrap();

    assert_eq!(store.get("ex-kinds").unwrap().events.len(), 4);
    let replay = events.replay_after(0);
    assert_eq!(replay.events.len(), 4);
    assert!(
        replay
            .events
            .iter()
            .all(|event| event.entity_id.as_deref() == Some("ex-kinds"))
    );
}

#[test]
fn consumer_owner_closes_sender_and_joins_after_draining_events() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let (layer, consumer) = ExchangeUiLayer::new(
        Arc::clone(&store),
        Arc::new(EventHub::new(64)),
        4,
        queue_budget(),
    )
    .expect("observation consumer thread");
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = "ex-shutdown",
            workspace_id = "10000000-0000-0000-0000-000000000001",
            listener_id = "20000000-0000-0000-0000-000000000002",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:9000",
            protocol = "socket"
        );
        let _entered = span.enter();
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
    });

    consumer.shutdown().unwrap();
    assert!(store.get("ex-shutdown").is_some());
}

fn wait_for_record(
    store: &ExchangeObservationStore,
    exchange_id: &str,
    event_count: usize,
) -> intercept_proxy_application::ExchangeObservationRecord {
    for _ in 0..100 {
        if let Some(record) = store.get(exchange_id)
            && record.events.len() >= event_count
        {
            return record;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("observation consumer did not persist {event_count} events")
}

#[test]
fn debug_fields_and_missing_open_metadata_are_not_parsed_or_guessed() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(4096),
    )));
    let (layer, consumer) = layer(&store);
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        let span = tracing::info_span!("exchange", exchange_id = "ex-debug");
        let _entered = span.enter();
        tracing::info!(
            target: "intercept_proxy::exchange::ui",
            event = "opened",
            workspace_id = ?"10000000-0000-0000-0000-000000000001",
            listener_id = ?"20000000-0000-0000-0000-000000000002",
            runtime_epoch = ?"30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:9000",
            protocol = "socket"
        );
    });
    consumer.shutdown().unwrap();

    for _ in 0..100 {
        if store.ignored_events() == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(store.get("ex-debug").is_none());
    assert_eq!(store.ignored_events(), 1);
}

#[test]
fn unrelated_targets_never_enter_exchange_store() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(4096),
    )));
    let (layer, consumer) = layer(&store);
    let subscriber = tracing_subscriber::registry().with(layer);
    with_default(subscriber, || {
        tracing::info!(
            target: "intercept_proxy::exchange::diagnostic",
            event = "opened",
            exchange_id = "ex-noise"
        );
    });
    consumer.shutdown().unwrap();
    assert!(store.get("ex-noise").is_none());
}

#[test]
fn http_failure_keeps_typed_context_and_available_error_fields() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let (layer, consumer) = layer(&store);
    let subscriber = tracing_subscriber::registry().with(layer);
    let workspace = "10000000-0000-0000-0000-000000000001";
    let listener = "20000000-0000-0000-0000-000000000002";
    let epoch = "30000000-0000-0000-0000-000000000003";
    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = 42_u128,
            workspace_id = workspace,
            listener_id = listener,
            runtime_epoch = epoch,
            peer = "127.0.0.1:8080",
            protocol = "http"
        );
        let _entered = span.enter();
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
        tracing::error!(
            target: "intercept_proxy::exchange::ui",
            event = "failed",
            direction = "downstream",
            stage = "write",
            context_header = "HTTP/1.1 500\r\nContent-Length: 4\r\n\r\n",
            context_body = "fail",
            context_body_is_utf8 = true,
            error = "peer reset",
            external_package_id = "phase10.http",
            external_package_version = "1.0.0",
            external_stage = "decode",
            external_method = "hooks.downstream.decode",
            external_request_id = "capture-9",
            external_remote_code = -32411_i64,
            external_stable_code = "BODY_DECODE_FAILED",
            external_remote_message = "decode rejected",
            external_remote_data_summary = "object(fields=1)"
        );
    });
    consumer.shutdown().unwrap();

    let record = wait_for_record(&store, "42", 2);
    let ExchangeObservationEvent::Failed {
        context,
        error,
        external_package_call,
        ..
    } = &record.events[1]
    else {
        panic!("failure event expected");
    };
    assert_eq!(error, "peer reset");
    let external = external_package_call
        .as_ref()
        .expect("typed external failure");
    assert_eq!(external.method, "hooks.downstream.decode");
    assert_eq!(external.request_id.as_deref(), Some("capture-9"));
    assert_eq!(external.remote_code, Some(-32411));
    assert_eq!(external.stable_code.as_deref(), Some("BODY_DECODE_FAILED"));
    assert_eq!(
        context,
        &Some(ExchangeContext::Http {
            header: "HTTP/1.1 500\r\nContent-Length: 4\r\n\r\n".to_owned(),
            body: "fail".to_owned(),
            body_is_utf8: true,
        })
    );
}

#[test]
fn two_transactions_append_to_the_same_connection_timeline() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let (layer, consumer) = layer(&store);
    let subscriber = tracing_subscriber::registry().with(layer);
    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = "ex-two",
            workspace_id = "10000000-0000-0000-0000-000000000001",
            listener_id = "20000000-0000-0000-0000-000000000002",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:9000",
            protocol = "socket"
        );
        let _entered = span.enter();
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
        for bytes in ["01", "02"] {
            tracing::info!(
                target: "intercept_proxy::exchange::ui",
                event = "received",
                direction = "upstream",
                context_bytes_hex = bytes,
            );
            tracing::info!(
                target: "intercept_proxy::exchange::ui",
                event = "sent",
                direction = "upstream",
                context_bytes_hex = bytes,
            );
        }
        tracing::info!(
            target: "intercept_proxy::exchange::ui",
            event = "closed",
            outcome = "completed"
        );
    });
    consumer.shutdown().unwrap();

    let record = wait_for_record(&store, "ex-two", 6);
    assert_eq!(record.events.len(), 6);
    assert!(matches!(
        record.events[0],
        ExchangeObservationEvent::Opened { .. }
    ));
    assert!(matches!(
        record.events[5],
        ExchangeObservationEvent::Closed { .. }
    ));
}
