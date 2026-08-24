//! tracing formatting 队列的 fail-open 丢弃计数。

use std::sync::atomic::{AtomicU64, Ordering};

use super::nonblocking_queue::QueueDropReason;

#[derive(Debug, Default)]
pub(super) struct RuntimeLogQueueCounters {
    full: AtomicU64,
    disconnected: AtomicU64,
    contended: AtomicU64,
}

impl RuntimeLogQueueCounters {
    pub(super) fn note_dropped(&self, reason: QueueDropReason) {
        let counter = match reason {
            QueueDropReason::Full | QueueDropReason::BytesFull => &self.full,
            QueueDropReason::Disconnected => &self.disconnected,
            QueueDropReason::Contended => &self.contended,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(super) fn full(&self) -> u64 {
        self.full.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(super) fn disconnected(&self) -> u64 {
        self.disconnected.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(super) fn contended(&self) -> u64 {
        self.contended.load(Ordering::Relaxed)
    }
}
