//! 生产者队列的 fail-open 与双重容量合同。

use std::{
    sync::{Arc, mpsc::sync_channel},
    time::Duration,
};

use intercept_proxy_application::{CapacityLedger, EventHub, UiEventPayload};
use intercept_proxy_infrastructure::{ExchangeObservationCounters, ExchangeObservationStore};
use tracing::subscriber::with_default;
use tracing_subscriber::prelude::*;

use super::queue_budget;
use crate::runtime_logs::{
    exchange_ui_layer::ExchangeUiLayer,
    nonblocking_queue::{BoundedMessage, BoundedSender, QueueByteBudget},
};

struct Occupier(usize);

impl BoundedMessage for Occupier {
    fn logical_bytes(&self) -> usize {
        self.0
    }
}

#[test]
fn blocked_consumer_and_full_queue_never_block_tracing_callback() {
    let (sender, receiver) = sync_channel(1);
    let counters = Arc::new(ExchangeObservationCounters::default());
    let (layer, _loss_receiver) =
        ExchangeUiLayer::from_sender_for_test(sender, Arc::clone(&counters), queue_budget());
    let subscriber = tracing_subscriber::registry().with(layer);
    let (consumed_signal, consumed_wait) = std::sync::mpsc::channel();
    let (release_signal, release_wait) = std::sync::mpsc::channel();
    let consumer = std::thread::spawn(move || {
        receiver.recv().expect("first event");
        consumed_signal.send(()).unwrap();
        release_wait.recv().unwrap();
    });

    with_default(subscriber, || {
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "first");
        consumed_wait.recv_timeout(Duration::from_secs(1)).unwrap();
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "second");
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "third");
    });

    assert_eq!(counters.dropped_events(), 1);
    release_signal.send(()).unwrap();
    consumer.join().unwrap();
}

#[test]
fn disconnected_observation_consumer_is_fail_open_and_counted() {
    let (sender, receiver) = sync_channel(1);
    drop(receiver);
    let counters = Arc::new(ExchangeObservationCounters::default());
    let (layer, _loss_receiver) =
        ExchangeUiLayer::from_sender_for_test(sender, Arc::clone(&counters), queue_budget());
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "after-shutdown");
    });

    assert_eq!(counters.dropped_events(), 1);
}

#[test]
fn byte_budget_rejects_large_observation_without_blocking_or_enqueueing() {
    let (sender, receiver) = sync_channel(1);
    let counters = Arc::new(ExchangeObservationCounters::default());
    let (layer, loss_receiver) = ExchangeUiLayer::from_sender_for_test(
        sender,
        Arc::clone(&counters),
        Arc::new(QueueByteBudget::new(128)),
    );
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        tracing::info!(
            target: "intercept_proxy::exchange::ui",
            event = "received",
            exchange_id = "large",
            protocol = "socket",
            direction = "upstream",
            context_bytes_hex = "AB".repeat(128),
        );
    });

    assert_eq!(counters.dropped_events(), 1);
    assert!(
        receiver.try_recv().is_err(),
        "oversized event is not queued"
    );
    assert!(
        loss_receiver.try_recv().is_ok(),
        "loss uses its control lane"
    );
}

#[test]
fn byte_budget_b_and_b_plus_one_leave_business_result_unchanged() {
    let (channel, receiver) = sync_channel(2);
    let shared_budget = Arc::new(QueueByteBudget::new(128));
    let sender = BoundedSender::from_sync_sender(channel, Arc::clone(&shared_budget));

    assert_eq!(sender.try_send(Occupier(128)), Ok(()));
    let business_result = || {
        let observation_result = sender.try_send(Occupier(1));
        assert_eq!(
            observation_result,
            Err(crate::runtime_logs::nonblocking_queue::QueueDropReason::BytesFull)
        );
        Result::<_, &'static str>::Ok("business-completed")
    };

    assert_eq!(business_result(), Ok("business-completed"));
    assert_eq!(
        receiver.try_iter().count(),
        1,
        "B is admitted and B+1 drops"
    );
}

#[test]
fn oversized_last_event_publishes_loss_refresh_from_consumer_thread() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let events = Arc::new(EventHub::new(16));
    let (layer, consumer) = ExchangeUiLayer::new(
        Arc::clone(&store),
        Arc::clone(&events),
        4,
        Arc::new(QueueByteBudget::new(128)),
    )
    .unwrap();
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        tracing::info!(
            target: "intercept_proxy::exchange::ui",
            event = "received",
            exchange_id = "large-last",
            protocol = "socket",
            direction = "upstream",
            context_bytes_hex = "CD".repeat(128),
        );
    });
    consumer.shutdown().unwrap();

    assert_eq!(store.dropped_events(), 1);
    assert_eq!(store.ignored_events(), 0);
    let replay = events.replay_after(0);
    assert_eq!(replay.events.len(), 1);
    assert!(matches!(
        replay.events[0].payload,
        UiEventPayload::ExchangeObservationChanged
    ));
    assert!(replay.events[0].entity_id.is_none());
}

#[test]
fn runtime_payload_pressure_cannot_consume_exchange_loss_refresh_lane() {
    let shared_budget = Arc::new(QueueByteBudget::new(128));
    let (occupier_channel, _occupier_receiver) = sync_channel(1);
    let occupier = BoundedSender::from_sync_sender(occupier_channel, Arc::clone(&shared_budget));
    occupier.try_send(Occupier(128)).unwrap();

    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let events = Arc::new(EventHub::new(16));
    let (layer, consumer) =
        ExchangeUiLayer::new(Arc::clone(&store), Arc::clone(&events), 4, shared_budget).unwrap();
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        tracing::info!(
            target: "intercept_proxy::exchange::ui",
            event = "observation_lost",
            reason = "shared_budget_full",
        );
    });
    consumer.shutdown().unwrap();

    assert_eq!(store.dropped_events(), 1);
    assert_eq!(store.ignored_events(), 0);
    let replay = events.replay_after(0);
    assert_eq!(replay.events.len(), 1);
    assert!(matches!(
        replay.events[0].payload,
        UiEventPayload::ExchangeObservationChanged
    ));
}
