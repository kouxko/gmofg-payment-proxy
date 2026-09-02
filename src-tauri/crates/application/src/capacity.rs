//! 会话与实时事件共用的内存容量账本。
//!
//! 两类数据存放在不同容器，却受同一个上限约束。本模块原子地预留和释放逻辑字节，
//! 避免两个容器分别检查后同时写入而突破总容量。

use parking_lot::Mutex;

/// 所有应用内存数据共用的字节容量权威。
///
/// 会话和 UI 事件写入各自容器前必须先预留实际
/// 保留字节。账本串行化准入判断，避免任一方使用其他容器的旧快照通过检查。
#[derive(Debug)]
pub struct CapacityLedger {
    state: Mutex<CapacityState>,
}

#[derive(Debug, Clone, Copy)]
struct CapacityState {
    limit: u64,
    sessions: u64,
    events: u64,
    observations: u64,
}

impl CapacityLedger {
    #[must_use]
    pub fn new(max_bytes: u64) -> Self {
        Self {
            state: Mutex::new(CapacityState {
                limit: max_bytes.max(1),
                sessions: 0,
                events: 0,
                observations: 0,
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
        state
            .sessions
            .saturating_add(state.events)
            .saturating_add(state.observations)
    }

    #[must_use]
    pub(crate) fn event_bytes(&self) -> u64 {
        self.state.lock().events
    }

    /// Returns the bytes reserved by the Exchange observation store.
    #[must_use]
    pub fn capture_bytes(&self) -> u64 {
        self.state.lock().observations
    }

    /// Atomically replaces the session allocation while preserving the current
    /// limit. This is the session store's only byte-admission operation.
    pub(crate) fn try_set_session_bytes(&self, bytes: u64) -> bool {
        let mut state = self.state.lock();
        if bytes
            .saturating_add(state.events)
            .saturating_add(state.observations)
            > state.limit
        {
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
        if bytes
            .saturating_add(state.events)
            .saturating_add(state.observations)
            > max_bytes
        {
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
            .saturating_add(state.observations)
            .saturating_add(bytes);
        if next > state.limit {
            return false;
        }
        state.events = state.events.saturating_add(bytes);
        true
    }

    /// Atomically transfers an existing event reservation to replacement data.
    ///
    /// Capture batching uses this when pending rows become one event envelope.
    /// The old reservation remains intact if the larger replacement cannot be
    /// admitted, so concurrent session admission can never consume a transient
    /// release/re-reserve gap.
    pub(crate) fn try_replace_event_bytes(&self, current: u64, replacement: u64) -> bool {
        let mut state = self.state.lock();
        if current > state.events {
            return false;
        }
        let retained_events = state.events.saturating_sub(current);
        let next = state
            .sessions
            .saturating_add(retained_events)
            .saturating_add(state.observations)
            .saturating_add(replacement);
        if next > state.limit {
            return false;
        }
        state.events = retained_events.saturating_add(replacement);
        true
    }

    pub(crate) fn release_event_bytes(&self, bytes: u64) {
        let mut state = self.state.lock();
        state.events = state.events.saturating_sub(bytes);
    }

    /// Reserves capacity for the in-memory Exchange observation stream.
    pub fn try_reserve_capture_bytes(&self, bytes: u64) -> bool {
        let mut state = self.state.lock();
        let next = state
            .sessions
            .saturating_add(state.events)
            .saturating_add(state.observations)
            .saturating_add(bytes);
        if next > state.limit {
            return false;
        }
        state.observations = state.observations.saturating_add(bytes);
        true
    }

    /// Atomically replaces this observation store's complete reservation.
    ///
    /// The old reservation remains intact when another memory owner wins capacity between the
    /// store's reclamation plan and this commit. Observation eviction can therefore be applied to
    /// the in-memory deque only after this method succeeds.
    pub fn try_replace_capture_bytes(&self, current: u64, replacement: u64) -> bool {
        let mut state = self.state.lock();
        if current > state.observations {
            return false;
        }
        let retained_observations = state.observations.saturating_sub(current);
        let next = state
            .sessions
            .saturating_add(state.events)
            .saturating_add(retained_observations)
            .saturating_add(replacement);
        if next > state.limit {
            return false;
        }
        state.observations = retained_observations.saturating_add(replacement);
        true
    }

    /// Releases bytes evicted from the in-memory Exchange observation stream.
    pub fn release_capture_bytes(&self, bytes: u64) {
        let mut state = self.state.lock();
        state.observations = state.observations.saturating_sub(bytes);
    }
}

impl Default for CapacityLedger {
    fn default() -> Self {
        Self::new(256 * 1024 * 1024)
    }
}
