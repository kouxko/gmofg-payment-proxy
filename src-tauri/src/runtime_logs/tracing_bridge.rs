use std::{io, sync::Arc};

use tracing::{Level, Metadata};
use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    fmt::MakeWriter,
    prelude::*,
};

use super::{ApplicationLogLevel, RuntimeLogStore};

#[derive(Debug, Clone)]
struct RuntimeLogWriterFactory {
    store: Arc<RuntimeLogStore>,
}

impl<'writer> MakeWriter<'writer> for RuntimeLogWriterFactory {
    type Writer = RuntimeLogWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        RuntimeLogWriter::new(
            Arc::clone(&self.store),
            ApplicationLogLevel::Info,
            "tracing",
        )
    }

    fn make_writer_for(&'writer self, metadata: &Metadata<'_>) -> Self::Writer {
        RuntimeLogWriter::new(
            Arc::clone(&self.store),
            level(*metadata.level()),
            metadata.target(),
        )
    }
}

#[derive(Debug)]
struct RuntimeLogWriter {
    store: Arc<RuntimeLogStore>,
    level: ApplicationLogLevel,
    target: String,
    buffer: Vec<u8>,
}

impl RuntimeLogWriter {
    fn new(store: Arc<RuntimeLogStore>, level: ApplicationLogLevel, target: &str) -> Self {
        Self {
            store,
            level,
            target: target.to_owned(),
            buffer: Vec::with_capacity(256),
        }
    }
}

impl io::Write for RuntimeLogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for RuntimeLogWriter {
    fn drop(&mut self) {
        let message = String::from_utf8_lossy(&self.buffer);
        let message = message.trim();
        if !message.is_empty() {
            self.store.record(self.level, &self.target, message);
        }
    }
}

/// Installs the single process-wide tracing subscriber before Tauri starts.
///
/// Formatting stays compact because level and module are stored as typed fields by
/// [`RuntimeLogStore`]. The subscriber deliberately omits `tracing-log`, preventing
/// the Tauri `log` target from being reflected back into this writer.
pub(crate) fn install_tracing_bridge(
    store: Arc<RuntimeLogStore>,
) -> Result<(), tracing::subscriber::SetGlobalDefaultError> {
    tracing::subscriber::set_global_default(runtime_subscriber(store))
}

fn runtime_subscriber(store: Arc<RuntimeLogStore>) -> impl tracing::Subscriber + Send + Sync {
    let filter = Targets::new()
        .with_default(LevelFilter::INFO)
        .with_target("intercept_proxy", LevelFilter::DEBUG);
    let formatting = tracing_subscriber::fmt::layer()
        .with_writer(RuntimeLogWriterFactory { store })
        .with_ansi(false)
        .with_level(false)
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .compact();
    tracing_subscriber::registry().with(formatting).with(filter)
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
    use tracing::subscriber::with_default;

    use super::*;
    use crate::runtime_logs::ApplicationLogQuery;

    #[test]
    fn tracing_fields_are_captured_with_level_and_target() {
        let store = Arc::new(RuntimeLogStore::memory(8));
        let subscriber = runtime_subscriber(Arc::clone(&store));

        with_default(subscriber, || {
            tracing::error!(
                listener_id = "entry-1",
                connection_id = "connection-1",
                error = "peer closed",
                "socket write failed"
            );
        });

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
        let subscriber = runtime_subscriber(Arc::clone(&store));

        with_default(subscriber, || {
            tracing::trace!(target: "tungstenite::protocol", "frame polling noise");
            tracing::debug!(target: "intercept_proxy::runtime", "diagnostic state transition");
        });

        let page = store.query(&ApplicationLogQuery::default());
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].level, ApplicationLogLevel::Debug);
        assert_eq!(page.rows[0].target, "intercept_proxy::runtime");
    }
}
