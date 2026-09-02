use std::{io, sync::Arc};

use chrono::{DateTime, Utc};

use tracing::{Level, Metadata};
use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    fmt::MakeWriter,
    prelude::*,
};

use super::{
    ApplicationLogLevel, RuntimeLogStore,
    exchange_ui_layer::{ExchangeUiConsumer, ExchangeUiLayer},
    nonblocking_queue::{
        BoundedMessage, BoundedSender, ConsumerOwner, QueueByteBudget, spawn_bounded_consumer,
    },
    runtime_log_counters::RuntimeLogQueueCounters,
    store::MAX_LOG_MESSAGE_CHARS,
};
use intercept_proxy_infrastructure::ExchangeObservationStore;

#[derive(Debug, Clone)]
struct RuntimeLogWriterFactory {
    sender: BoundedSender<RuntimeLogMessage>,
    counters: Arc<RuntimeLogQueueCounters>,
}

impl RuntimeLogWriterFactory {
    #[cfg(test)]
    fn from_sender_for_test(
        sender: std::sync::mpsc::SyncSender<super::nonblocking_queue::Budgeted<RuntimeLogMessage>>,
        counters: Arc<RuntimeLogQueueCounters>,
        queue_budget: Arc<QueueByteBudget>,
    ) -> Self {
        Self {
            sender: BoundedSender::from_sync_sender(sender, queue_budget),
            counters,
        }
    }
}

impl<'writer> MakeWriter<'writer> for RuntimeLogWriterFactory {
    type Writer = RuntimeLogWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        RuntimeLogWriter::new(
            self.sender.clone(),
            Arc::clone(&self.counters),
            ApplicationLogLevel::Info,
            "tracing",
        )
    }

    fn make_writer_for(&'writer self, metadata: &Metadata<'_>) -> Self::Writer {
        RuntimeLogWriter::new(
            self.sender.clone(),
            Arc::clone(&self.counters),
            level(*metadata.level()),
            metadata.target(),
        )
    }
}

#[derive(Debug)]
struct RuntimeLogWriter {
    sender: BoundedSender<RuntimeLogMessage>,
    counters: Arc<RuntimeLogQueueCounters>,
    level: ApplicationLogLevel,
    target: String,
    buffer: Vec<u8>,
    truncated: bool,
}

#[derive(Debug)]
struct RuntimeLogMessage {
    level: ApplicationLogLevel,
    target: String,
    message: String,
    occurred_at: DateTime<Utc>,
}

impl RuntimeLogWriter {
    fn new(
        sender: BoundedSender<RuntimeLogMessage>,
        counters: Arc<RuntimeLogQueueCounters>,
        level: ApplicationLogLevel,
        target: &str,
    ) -> Self {
        Self {
            sender,
            counters,
            level,
            target: target.to_owned(),
            buffer: Vec::with_capacity(256),
            truncated: false,
        }
    }
}

impl io::Write for RuntimeLogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        const MAX_QUEUED_LOG_BYTES: usize = MAX_LOG_MESSAGE_CHARS * 4;
        let remaining = MAX_QUEUED_LOG_BYTES.saturating_sub(self.buffer.len());
        let accepted = remaining.min(bytes.len());
        self.buffer.extend_from_slice(&bytes[..accepted]);
        self.truncated |= accepted < bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for RuntimeLogWriter {
    fn drop(&mut self) {
        let message = String::from_utf8_lossy(&self.buffer);
        let mut message = message.trim().to_owned();
        if self.truncated {
            message.push_str("…[truncated before queue]");
        }
        if !message.is_empty() {
            let queued = RuntimeLogMessage {
                level: self.level,
                target: self.target.clone(),
                message,
                occurred_at: Utc::now(),
            };
            if let Err(reason) = self.sender.try_send(queued) {
                self.counters.note_dropped(reason);
            }
        }
    }
}

impl BoundedMessage for RuntimeLogMessage {
    fn logical_bytes(&self) -> usize {
        self.target
            .len()
            .saturating_add(self.message.len())
            .saturating_add(64)
    }
}

#[derive(Debug)]
struct RuntimeLogConsumer(ConsumerOwner<RuntimeLogMessage>);

impl RuntimeLogConsumer {
    fn shutdown(&self) -> io::Result<()> {
        self.0.shutdown()
    }
}

/// Owns both tracing consumers installed in the process-wide subscriber.
///
/// The global subscriber cannot be dropped before process exit, so its sender slots are closed
/// explicitly through this owner and both consumer threads are joined during application shutdown.
#[derive(Debug)]
pub(crate) struct TracingBridge {
    runtime_logs: RuntimeLogConsumer,
    exchange_ui: ExchangeUiConsumer,
}

impl TracingBridge {
    pub(crate) fn shutdown(&self) -> io::Result<()> {
        let exchange_result = self.exchange_ui.shutdown();
        let runtime_result = self.runtime_logs.shutdown();
        exchange_result.and(runtime_result)
    }
}

/// Installs the single process-wide tracing subscriber before Tauri starts.
///
/// Formatting stays compact because level and module are stored as typed fields by
/// [`RuntimeLogStore`]. The subscriber deliberately omits `tracing-log`, preventing
/// the Tauri `log` target from being reflected back into this writer.
pub(crate) fn install_tracing_bridge(
    store: Arc<RuntimeLogStore>,
    exchange_observations: Arc<ExchangeObservationStore>,
    events: Arc<intercept_proxy_application::EventHub>,
    observation_queue_capacity: usize,
    observation_queue_bytes: usize,
) -> std::io::Result<TracingBridge> {
    let (subscriber, consumers) = runtime_subscriber(
        store,
        exchange_observations,
        events,
        observation_queue_capacity,
        observation_queue_bytes,
    )?;
    tracing::subscriber::set_global_default(subscriber).map_err(std::io::Error::other)?;
    Ok(consumers)
}

fn runtime_subscriber(
    store: Arc<RuntimeLogStore>,
    exchange_observations: Arc<ExchangeObservationStore>,
    events: Arc<intercept_proxy_application::EventHub>,
    observation_queue_capacity: usize,
    observation_queue_bytes: usize,
) -> std::io::Result<(impl tracing::Subscriber + Send + Sync, TracingBridge)> {
    let filter = Targets::new()
        .with_default(LevelFilter::INFO)
        .with_target("intercept_proxy", LevelFilter::DEBUG);
    let counters = store.queue_counters();
    let queue_budget = Arc::new(QueueByteBudget::new(observation_queue_bytes));
    let (runtime_sender, runtime_consumer) = spawn_bounded_consumer(
        "runtime-log",
        observation_queue_capacity,
        Arc::clone(&queue_budget),
        move |message: RuntimeLogMessage| {
            store.record_at(
                message.level,
                &message.target,
                &message.message,
                message.occurred_at,
            );
        },
    )?;
    let formatting = tracing_subscriber::fmt::layer()
        .with_writer(RuntimeLogWriterFactory {
            sender: runtime_sender,
            counters,
        })
        .with_ansi(false)
        .with_level(false)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .compact();
    let (exchange_ui, exchange_consumer) = ExchangeUiLayer::new(
        exchange_observations,
        events,
        observation_queue_capacity,
        queue_budget,
    )?;
    let subscriber = tracing_subscriber::registry()
        .with(formatting)
        .with(exchange_ui)
        .with(filter);
    Ok((
        subscriber,
        TracingBridge {
            runtime_logs: RuntimeLogConsumer(runtime_consumer),
            exchange_ui: exchange_consumer,
        },
    ))
}

fn level(level: Level) -> ApplicationLogLevel {
    match level {
        Level::TRACE => ApplicationLogLevel::Trace,
        Level::DEBUG => ApplicationLogLevel::Debug,
        Level::INFO => ApplicationLogLevel::Info,
        Level::WARN => ApplicationLogLevel::Warning,
        Level::ERROR => ApplicationLogLevel::Error,
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Write as _, sync::mpsc::sync_channel};

    use intercept_proxy_application::CapacityLedger;
    use tracing::subscriber::with_default;

    use super::*;
    use crate::runtime_logs::ApplicationLogQuery;

    #[test]
    fn tracing_fields_are_captured_with_level_and_target() {
        let store = Arc::new(RuntimeLogStore::memory(8));
        let (subscriber, consumers) =
            runtime_subscriber(Arc::clone(&store), observations(), events(), 32, 64 * 1024)
                .unwrap();

        with_default(subscriber, || {
            tracing::error!(
                listener_id = "entry-1",
                connection_id = "connection-1",
                error = "peer closed",
                "socket write failed"
            );
        });
        consumers.shutdown().unwrap();

        let page = store.query(&ApplicationLogQuery::default());
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].level, ApplicationLogLevel::Error);
        assert!(page.rows[0].target.ends_with("tracing_bridge::tests"));
        for evidence in [
            "entry-1",
            "connection-1",
            "peer closed",
            "socket write failed",
        ] {
            assert!(page.rows[0].message.contains(evidence));
        }
    }

    #[test]
    fn default_runtime_subscriber_excludes_dependency_trace_noise() {
        let store = Arc::new(RuntimeLogStore::memory(8));
        let (subscriber, consumers) =
            runtime_subscriber(Arc::clone(&store), observations(), events(), 32, 64 * 1024)
                .unwrap();

        with_default(subscriber, || {
            tracing::trace!(target: "tungstenite::protocol", "frame polling noise");
            tracing::debug!(target: "intercept_proxy::runtime", "diagnostic state transition");
        });
        consumers.shutdown().unwrap();

        let page = store.query(&ApplicationLogQuery::default());
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].level, ApplicationLogLevel::Debug);
        assert_eq!(page.rows[0].target, "intercept_proxy::runtime");
    }

    #[test]
    fn formatting_writer_fails_open_when_queue_is_full_or_disconnected() {
        let store = Arc::new(RuntimeLogStore::memory(8));
        let (sender, receiver) = sync_channel(1);
        let counters = store.queue_counters();
        let factory = RuntimeLogWriterFactory::from_sender_for_test(
            sender,
            Arc::clone(&counters),
            Arc::new(QueueByteBudget::new(64 * 1024)),
        );

        let mut first = factory.make_writer();
        first.write_all(b"first").unwrap();
        drop(first);

        let mut full = factory.make_writer();
        full.write_all(b"full").unwrap();
        drop(full);
        assert_eq!(counters.full(), 1);

        drop(receiver);
        let mut disconnected = factory.make_writer();
        disconnected.write_all(b"disconnected").unwrap();
        drop(disconnected);
        assert_eq!(counters.disconnected(), 1);

        assert_eq!(counters.contended(), 0);
    }

    #[test]
    fn runtime_log_queue_losses_are_exposed_by_the_query_contract() {
        let store = Arc::new(RuntimeLogStore::memory(8));
        let (sender, receiver) = sync_channel(1);
        let counters = store.queue_counters();
        let factory = RuntimeLogWriterFactory::from_sender_for_test(
            sender,
            Arc::clone(&counters),
            Arc::new(QueueByteBudget::new(64 * 1024)),
        );

        let mut first = factory.make_writer();
        first.write_all(b"first").unwrap();
        drop(first);
        let mut dropped = factory.make_writer();
        dropped.write_all(b"dropped").unwrap();
        drop(dropped);
        drop(receiver);
        let mut disconnected = factory.make_writer();
        disconnected.write_all(b"disconnected").unwrap();
        drop(disconnected);

        let page = store.query(&ApplicationLogQuery::default());
        assert_eq!(page.queue_dropped_full, 1);
        assert_eq!(page.queue_dropped_disconnected, 1);
        assert_eq!(page.queue_dropped_contended, 0);
    }

    #[test]
    fn formatting_writer_bounds_message_before_queue_admission() {
        let store = Arc::new(RuntimeLogStore::memory(8));
        let (sender, receiver) = sync_channel(1);
        let counters = store.queue_counters();
        let factory = RuntimeLogWriterFactory::from_sender_for_test(
            sender,
            counters,
            Arc::new(QueueByteBudget::new(MAX_LOG_MESSAGE_CHARS * 4 + 256)),
        );
        let mut writer = factory.make_writer();

        writer
            .write_all(&vec![b'A'; MAX_LOG_MESSAGE_CHARS * 8])
            .unwrap();
        drop(writer);

        let queued = receiver.recv().unwrap();
        assert!(queued.message.message.len() <= MAX_LOG_MESSAGE_CHARS * 4 + 32);
        assert!(queued.message.message.ends_with("[truncated before queue]"));
    }

    fn observations() -> Arc<ExchangeObservationStore> {
        Arc::new(ExchangeObservationStore::new(Arc::new(
            CapacityLedger::new(64 * 1024),
        )))
    }

    fn events() -> Arc<intercept_proxy_application::EventHub> {
        Arc::new(intercept_proxy_application::EventHub::new(64))
    }
}
