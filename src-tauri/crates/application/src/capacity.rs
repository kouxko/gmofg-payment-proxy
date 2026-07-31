use parking_lot::Mutex;

/// Shared byte-capacity authority for all in-memory application data.
///
/// Sessions and UI events must reserve their actual retained bytes here before
/// committing them to their own containers. The ledger serializes admission,
/// so neither side can pass a check using a stale snapshot of the other side.
#[derive(Debug)]
pub struct CapacityLedger {
    state: Mutex<CapacityState>,
}

#[derive(Debug, Clone, Copy)]
struct CapacityState {
    limit: u64,
    sessions: u64,
    events: u64,
}

impl CapacityLedger {
    #[must_use]
    pub fn new(max_bytes: u64) -> Self {
        Self {
            state: Mutex::new(CapacityState {
                limit: max_bytes.max(1),
                sessions: 0,
                events: 0,
            }),
        }
    }

    #[must_use]
    pub fn max_bytes(&self) -> u64 {
        self.state.lock().limit
    }

    #[must_use]
    pub fn logical_bytes(&self) -> u64 {
        let state = self.state.lock();
        state.sessions.saturating_add(state.events)
    }

    #[must_use]
    pub(crate) fn event_bytes(&self) -> u64 {
        self.state.lock().events
    }

    /// Atomically replaces the session allocation while preserving the current
    /// limit. This is the session store's only byte-admission operation.
    pub(crate) fn try_set_session_bytes(&self, bytes: u64) -> bool {
        let mut state = self.state.lock();
        if bytes.saturating_add(state.events) > state.limit {
            return false;
        }
        state.sessions = bytes;
        true
    }

    /// Atomically changes both the configured limit and session allocation.
    ///
    /// Updating both values together avoids a transient overcommit when the
    /// settings use case lowers the limit and evicts completed sessions.
    pub(crate) fn try_set_session_bytes_and_limit(&self, bytes: u64, max_bytes: u64) -> bool {
        let max_bytes = max_bytes.max(1);
        let mut state = self.state.lock();
        if bytes.saturating_add(state.events) > max_bytes {
            return false;
        }
        state.sessions = bytes;
        state.limit = max_bytes;
        true
    }

    /// Reserves event memory without blocking network processing.
    ///
    /// Callers reclaim replay or subscriber storage and retry when this returns
    /// `false`; the ledger never records an over-capacity intermediate state.
    pub(crate) fn try_reserve_event_bytes(&self, bytes: u64) -> bool {
        let mut state = self.state.lock();
        let next = state
            .sessions
            .saturating_add(state.events)
            .saturating_add(bytes);
        if next > state.limit {
            return false;
        }
        state.events = state.events.saturating_add(bytes);
        true
    }

    pub(crate) fn release_event_bytes(&self, bytes: u64) {
        let mut state = self.state.lock();
        state.events = state.events.saturating_sub(bytes);
    }
}

impl Default for CapacityLedger {
    fn default() -> Self {
        Self::new(256 * 1024 * 1024)
    }
}
