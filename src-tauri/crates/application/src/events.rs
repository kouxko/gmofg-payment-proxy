use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    AppError, AppResult, CapacityLedger, CaptureRowViewModel, Revision, RuntimeEpoch,
    SubscriptionAckViewModel, UiEventEnvelope, UiEventPayload,
};

#[derive(Debug, Clone)]
pub struct EventReplay {
    pub events: Vec<UiEventEnvelope>,
    pub current_cursor: u64,
    pub snapshot_required: bool,
}

#[derive(Debug)]
pub struct EventSubscription {
    pub subscription_id: u64,
    pub ack: SubscriptionAckViewModel,
    pub replay: Vec<UiEventEnvelope>,
    pub live: TrackedEventReceiver,
}

#[derive(Debug)]
pub struct TrackedEventReceiver {
    receiver: mpsc::Receiver<UiEventEnvelope>,
    queued_logical_bytes: Arc<AtomicU64>,
    state: Weak<Mutex<EventState>>,
    capacity: Arc<CapacityLedger>,
    subscription_id: u64,
    cancellation: CancellationToken,
}

impl TrackedEventReceiver {
    pub async fn recv(&mut self) -> Option<UiEventEnvelope> {
        let event = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => return None,
            event = self.receiver.recv() => event?,
        };
        let released = subtract_saturating(&self.queued_logical_bytes, event.logical_bytes());
        self.capacity.release_event_bytes(released);
        Some(event)
    }
}

impl Drop for TrackedEventReceiver {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(state) = self.state.upgrade() {
            let mut state = state.lock();
            remove_subscriber(
                &mut state,
                self.capacity.as_ref(),
                self.subscription_id,
                true,
            );
            remove_subscription_failures(&mut state, self.capacity.as_ref(), self.subscription_id);
        }
    }
}

#[derive(Debug)]
pub struct EventHub {
    replay_capacity: usize,
    capacity: Arc<CapacityLedger>,
    state: Arc<Mutex<EventState>>,
}

#[derive(Debug)]
struct EventState {
    next_id: u64,
    retained: VecDeque<UiEventEnvelope>,
    pending_capture: Vec<CaptureRowViewModel>,
    capture_epoch: Option<RuntimeEpoch>,
    capture_started_at: Option<DateTime<Utc>>,
    replay_overflowed: bool,
    next_subscription_id: u64,
    subscribers: HashMap<u64, LiveSubscriber>,
    subscription_bytes: HashMap<u64, Arc<AtomicU64>>,
    subscription_failures: VecDeque<(u64, UiEventEnvelope)>,
}

#[derive(Debug)]
struct LiveSubscriber {
    sender: mpsc::Sender<UiEventEnvelope>,
    cancellation: CancellationToken,
}

struct PendingEvent {
    runtime_epoch: Option<RuntimeEpoch>,
    occurred_at: DateTime<Utc>,
    entity_id: Option<String>,
    entity_revision: Option<Revision>,
    payload: UiEventPayload,
}

impl EventHub {
    pub const DEFAULT_CAPACITY: usize = 4_096;
    pub const DEFAULT_SUBSCRIBER_CAPACITY: usize = 512;
    pub const DEFAULT_FAILURE_CAPACITY: usize = 512;
    pub const MAX_SUBSCRIBERS: usize = 16;
    pub const CAPTURE_BATCH_SIZE: usize = 200;
    pub const CAPTURE_BATCH_INTERVAL: Duration = Duration::from_millis(100);

    pub fn new(capacity: usize) -> Self {
        Self::with_capacity_ledger(
            capacity,
            Arc::new(CapacityLedger::new(
                crate::InMemorySessionStore::DEFAULT_MAX_BYTES,
            )),
        )
    }

    /// Creates an event hub using the process-wide byte-capacity authority.
    ///
    /// `replay_capacity` is a count ceiling only. Byte admission is always
    /// decided by `capacity`, including replay, pending batches, live queues,
    /// and retained delivery failures.
    #[must_use]
    pub fn with_capacity_ledger(replay_capacity: usize, capacity: Arc<CapacityLedger>) -> Self {
        Self {
            replay_capacity: replay_capacity.max(1),
            capacity,
            state: Arc::new(Mutex::new(EventState {
                next_id: 1,
                retained: VecDeque::new(),
                pending_capture: Vec::new(),
                capture_epoch: None,
                capture_started_at: None,
                replay_overflowed: false,
                next_subscription_id: 1,
                subscribers: HashMap::new(),
                subscription_bytes: HashMap::new(),
                subscription_failures: VecDeque::new(),
            })),
        }
    }

    pub fn publish(
        &self,
        runtime_epoch: Option<RuntimeEpoch>,
        occurred_at: DateTime<Utc>,
        entity_id: Option<String>,
        entity_revision: Option<Revision>,
        payload: UiEventPayload,
    ) -> UiEventEnvelope {
        let mut state = self.state.lock();
        let (envelope, warning) = Self::append_log(
            &mut state,
            self.replay_capacity,
            self.capacity.as_ref(),
            PendingEvent {
                runtime_epoch,
                occurred_at,
                entity_id,
                entity_revision,
                payload,
            },
        );
        Self::dispatch_live(&mut state, self.capacity.as_ref(), &envelope);
        if let Some(warning) = warning {
            Self::dispatch_live(&mut state, self.capacity.as_ref(), &warning);
        }
        envelope
    }

    pub fn push_capture(
        &self,
        runtime_epoch: RuntimeEpoch,
        occurred_at: DateTime<Utc>,
        row: CaptureRowViewModel,
    ) -> Option<UiEventEnvelope> {
        let mut state = self.state.lock();
        if state
            .capture_epoch
            .is_some_and(|epoch| epoch != runtime_epoch)
            && !state.pending_capture.is_empty()
        {
            let _ = Self::flush_capture_locked(
                &mut state,
                self.replay_capacity,
                self.capacity.as_ref(),
                occurred_at,
            );
        }
        state.capture_epoch = Some(runtime_epoch);
        state.capture_started_at.get_or_insert(occurred_at);
        let old_bytes = pending_capture_bytes(&state.pending_capture);
        state.pending_capture.push(row);
        let new_bytes = pending_capture_bytes(&state.pending_capture);
        let added = new_bytes.saturating_sub(old_bytes);
        if !reserve_with_reclamation(&mut state, self.capacity.as_ref(), added) {
            let row = state
                .pending_capture
                .pop()
                .expect("the just-appended capture row is present");
            if !state.pending_capture.is_empty() {
                let _ = Self::flush_capture_locked(
                    &mut state,
                    self.replay_capacity,
                    self.capacity.as_ref(),
                    occurred_at,
                );
            }
            let (direct, warning) = Self::append_log(
                &mut state,
                self.replay_capacity,
                self.capacity.as_ref(),
                PendingEvent {
                    runtime_epoch: Some(runtime_epoch),
                    occurred_at,
                    entity_id: None,
                    entity_revision: None,
                    payload: UiEventPayload::CaptureRowsAdded(vec![row]),
                },
            );
            Self::dispatch_live(&mut state, self.capacity.as_ref(), &direct);
            if let Some(warning) = warning {
                Self::dispatch_live(&mut state, self.capacity.as_ref(), &warning);
            }
            return Some(direct);
        }
        (state.pending_capture.len() >= Self::CAPTURE_BATCH_SIZE)
            .then(|| {
                Self::flush_capture_locked(
                    &mut state,
                    self.replay_capacity,
                    self.capacity.as_ref(),
                    occurred_at,
                )
            })
            .flatten()
    }

    pub fn flush_due(&self, now: DateTime<Utc>) -> Option<UiEventEnvelope> {
        let mut state = self.state.lock();
        let due = state.capture_started_at.is_some_and(|started| {
            now.signed_duration_since(started)
                .to_std()
                .is_ok_and(|elapsed| elapsed >= Self::CAPTURE_BATCH_INTERVAL)
        });
        due.then(|| {
            Self::flush_capture_locked(
                &mut state,
                self.replay_capacity,
                self.capacity.as_ref(),
                now,
            )
        })
        .flatten()
    }

    pub fn flush_capture(&self, now: DateTime<Utc>) -> Option<UiEventEnvelope> {
        Self::flush_capture_locked(
            &mut self.state.lock(),
            self.replay_capacity,
            self.capacity.as_ref(),
            now,
        )
    }

    pub fn replay_after(&self, after_event_id: u64) -> EventReplay {
        Self::replay_locked(&self.state.lock(), after_event_id)
    }

    /// Atomically captures the replay boundary and registers the independent live queue.
    ///
    /// The caller sends `replay` first and then drains `live`, preserving total event order.
    pub fn subscribe(
        &self,
        after_event_id: u64,
        queue_capacity: usize,
    ) -> AppResult<EventSubscription> {
        let mut state = self.state.lock();
        let replay = Self::replay_locked(&state, after_event_id);
        if !replay.snapshot_required && state.subscribers.len() >= Self::MAX_SUBSCRIBERS {
            return Err(
                AppError::new("RESOURCE_EXHAUSTED", "实时事件订阅数量已达到上限。")
                    .retryable("请关闭未使用的窗口后重试。"),
            );
        }
        let subscription_id = state.next_subscription_id;
        state.next_subscription_id = state.next_subscription_id.saturating_add(1);
        let (sender, receiver) =
            mpsc::channel(queue_capacity.clamp(1, Self::DEFAULT_SUBSCRIBER_CAPACITY));
        let queued_logical_bytes = Arc::new(AtomicU64::new(0));
        let cancellation = CancellationToken::new();
        if !replay.snapshot_required {
            state.subscribers.insert(
                subscription_id,
                LiveSubscriber {
                    sender,
                    cancellation: cancellation.clone(),
                },
            );
            state
                .subscription_bytes
                .insert(subscription_id, queued_logical_bytes.clone());
        }
        Ok(EventSubscription {
            subscription_id,
            ack: SubscriptionAckViewModel {
                subscription_id,
                accepted_after_event_id: after_event_id,
                current_event_id: replay.current_cursor,
                snapshot_required: replay.snapshot_required,
            },
            replay: replay.events,
            live: TrackedEventReceiver {
                receiver,
                queued_logical_bytes,
                state: Arc::downgrade(&self.state),
                capacity: Arc::clone(&self.capacity),
                subscription_id,
                cancellation,
            },
        })
    }

    pub fn subscribe_default(&self, after_event_id: u64) -> AppResult<EventSubscription> {
        self.subscribe(after_event_id, Self::DEFAULT_SUBSCRIBER_CAPACITY)
    }

    pub fn unsubscribe(&self, subscription_id: u64) {
        let mut state = self.state.lock();
        remove_subscriber(&mut state, self.capacity.as_ref(), subscription_id, true);
    }

    /// Returns non-blocking delivery failures for adapters to log or surface.
    pub fn take_subscription_failures(&self) -> Vec<(u64, UiEventEnvelope)> {
        let mut state = self.state.lock();
        let failures = state.subscription_failures.drain(..).collect::<Vec<_>>();
        let bytes = failures.iter().map(failure_logical_bytes).sum();
        self.capacity.release_event_bytes(bytes);
        failures
    }

    pub fn take_subscription_failure(&self, subscription_id: u64) -> Option<UiEventEnvelope> {
        let mut state = self.state.lock();
        let index = state
            .subscription_failures
            .iter()
            .position(|(id, _)| *id == subscription_id)?;
        state.subscription_failures.remove(index).map(|entry| {
            self.capacity
                .release_event_bytes(failure_logical_bytes(&entry));
            entry.1
        })
    }

    pub fn current_cursor(&self) -> u64 {
        self.state.lock().next_id.saturating_sub(1)
    }

    pub fn logical_bytes(&self) -> u64 {
        self.capacity.event_bytes()
    }

    /// Reclaims optional event storage before a shared limit is lowered.
    ///
    /// Pending capture rows are preserved. The caller can then ask the session
    /// store to atomically apply the new limit and evict only completed
    /// sessions; active sessions and pending breakpoints remain protected.
    pub fn reclaim_for_limit(&self, max_bytes: u64) -> bool {
        let mut state = self.state.lock();
        while self.capacity.logical_bytes() > max_bytes {
            if release_oldest_retained(&mut state, self.capacity.as_ref()) {
                state.replay_overflowed = true;
                continue;
            }
            if release_oldest_failure(&mut state, self.capacity.as_ref()) {
                continue;
            }
            let Some(subscription_id) = state.subscribers.keys().next().copied() else {
                break;
            };
            remove_subscriber(&mut state, self.capacity.as_ref(), subscription_id, true);
        }
        self.capacity.logical_bytes() <= max_bytes
    }

    /// Runs the 100 ms production flush clock without coupling the network
    /// path to UI subscriber speed.
    pub fn spawn_capture_flush_task(
        self: Arc<Self>,
        cancellation: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Self::CAPTURE_BATCH_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    _ = interval.tick() => {
                        let _ = self.flush_due(Utc::now());
                    }
                }
            }
            let _ = self.flush_capture(Utc::now());
        })
    }

    fn replay_locked(state: &EventState, after_event_id: u64) -> EventReplay {
        let current_cursor = state.next_id.saturating_sub(1);
        let oldest = state
            .retained
            .front()
            .map_or(state.next_id, |event| event.event_id);
        let snapshot_required =
            after_event_id < current_cursor && after_event_id.saturating_add(1) < oldest;
        if snapshot_required {
            EventReplay {
                events: vec![UiEventEnvelope {
                    event_id: current_cursor,
                    runtime_epoch: None,
                    occurred_at: Utc::now(),
                    entity_id: None,
                    entity_revision: None,
                    payload: UiEventPayload::SnapshotRequired {
                        reason: "事件游标已过期，请重新获取应用快照。".into(),
                    },
                }],
                current_cursor,
                snapshot_required: true,
            }
        } else {
            EventReplay {
                events: state
                    .retained
                    .iter()
                    .filter(|event| event.event_id > after_event_id)
                    .cloned()
                    .collect(),
                current_cursor,
                snapshot_required: false,
            }
        }
    }

    fn flush_capture_locked(
        state: &mut EventState,
        replay_capacity: usize,
        capacity: &CapacityLedger,
        occurred_at: DateTime<Utc>,
    ) -> Option<UiEventEnvelope> {
        if state.pending_capture.is_empty() {
            return None;
        }
        let pending_bytes = pending_capture_bytes(&state.pending_capture);
        let rows = std::mem::take(&mut state.pending_capture);
        capacity.release_event_bytes(pending_bytes);
        let epoch = state.capture_epoch.take();
        state.capture_started_at = None;
        let (envelope, warning) = Self::append_log(
            state,
            replay_capacity,
            capacity,
            PendingEvent {
                runtime_epoch: epoch,
                occurred_at,
                entity_id: None,
                entity_revision: None,
                payload: UiEventPayload::CaptureRowsAdded(rows),
            },
        );
        Self::dispatch_live(state, capacity, &envelope);
        if let Some(warning) = warning {
            Self::dispatch_live(state, capacity, &warning);
        }
        Some(envelope)
    }

    fn append_log(
        state: &mut EventState,
        replay_capacity: usize,
        capacity: &CapacityLedger,
        pending: PendingEvent,
    ) -> (UiEventEnvelope, Option<UiEventEnvelope>) {
        let envelope = UiEventEnvelope {
            event_id: state.next_id,
            runtime_epoch: pending.runtime_epoch,
            occurred_at: pending.occurred_at,
            entity_id: pending.entity_id,
            entity_revision: pending.entity_revision,
            payload: pending.payload,
        };
        state.next_id = state.next_id.saturating_add(1);
        let envelope_bytes = envelope.logical_bytes();
        let overflow_was_active = state.replay_overflowed;
        let retained = reserve_with_reclamation(state, capacity, envelope_bytes);
        if retained {
            state.retained.push_back(envelope.clone());
        }

        let mut overflow_started = !overflow_was_active && state.replay_overflowed;
        while state.retained.len() > replay_capacity {
            release_oldest_retained(state, capacity);
            overflow_started |= !state.replay_overflowed;
            state.replay_overflowed = true;
        }
        if !retained {
            overflow_started |= !state.replay_overflowed;
            state.replay_overflowed = true;
        }
        let warning = if overflow_started {
            let warning = UiEventEnvelope {
                event_id: state.next_id,
                runtime_epoch: envelope.runtime_epoch,
                occurred_at: envelope.occurred_at,
                entity_id: None,
                entity_revision: None,
                payload: UiEventPayload::ResourceWarning {
                    message: "UI 补发日志已淘汰旧事件；过期订阅需要重新获取快照。".into(),
                },
            };
            state.next_id = state.next_id.saturating_add(1);
            let warning_bytes = warning.logical_bytes();
            if reserve_with_reclamation(state, capacity, warning_bytes) {
                state.retained.push_back(warning.clone());
            }
            while state.retained.len() > replay_capacity {
                release_oldest_retained(state, capacity);
            }
            Some(warning)
        } else {
            None
        };
        (envelope, warning)
    }

    fn dispatch_live(
        state: &mut EventState,
        capacity: &CapacityLedger,
        envelope: &UiEventEnvelope,
    ) {
        let mut terminated = Vec::new();
        for (subscription_id, subscriber) in &state.subscribers {
            let logical_bytes = envelope.logical_bytes();
            let Some(queued_bytes) = state.subscription_bytes.get(subscription_id) else {
                terminated.push(*subscription_id);
                continue;
            };
            if !capacity.try_reserve_event_bytes(logical_bytes) {
                terminated.push(*subscription_id);
                continue;
            }
            queued_bytes.fetch_add(logical_bytes, Ordering::Relaxed);
            if subscriber.sender.try_send(envelope.clone()).is_err() {
                let released = subtract_saturating(queued_bytes, logical_bytes);
                capacity.release_event_bytes(released);
                terminated.push(*subscription_id);
            }
        }
        for subscription_id in terminated {
            remove_subscriber(state, capacity, subscription_id, true);
            let failure = (
                subscription_id,
                UiEventEnvelope {
                    event_id: envelope.event_id,
                    runtime_epoch: envelope.runtime_epoch,
                    occurred_at: envelope.occurred_at,
                    entity_id: None,
                    entity_revision: None,
                    payload: UiEventPayload::SnapshotRequired {
                        reason: "实时订阅队列已满，订阅已终止；请重新获取应用快照。".into(),
                    },
                },
            );
            let failure_bytes = failure_logical_bytes(&failure);
            if reserve_with_reclamation(state, capacity, failure_bytes) {
                state.subscription_failures.push_back(failure);
            }
            while state.subscription_failures.len() > Self::DEFAULT_FAILURE_CAPACITY {
                release_oldest_failure(state, capacity);
            }
        }
    }
}

fn pending_capture_bytes(rows: &[CaptureRowViewModel]) -> u64 {
    if rows.is_empty() {
        0
    } else {
        serde_json::to_vec(rows).map_or(0, |bytes| bytes.len() as u64)
    }
}

fn failure_logical_bytes(failure: &(u64, UiEventEnvelope)) -> u64 {
    8_u64.saturating_add(failure.1.logical_bytes())
}

fn release_oldest_retained(state: &mut EventState, capacity: &CapacityLedger) -> bool {
    let Some(event) = state.retained.pop_front() else {
        return false;
    };
    capacity.release_event_bytes(event.logical_bytes());
    true
}

fn release_oldest_failure(state: &mut EventState, capacity: &CapacityLedger) -> bool {
    let Some(failure) = state.subscription_failures.pop_front() else {
        return false;
    };
    capacity.release_event_bytes(failure_logical_bytes(&failure));
    true
}

fn remove_subscription_failures(
    state: &mut EventState,
    capacity: &CapacityLedger,
    subscription_id: u64,
) {
    let mut retained = VecDeque::with_capacity(state.subscription_failures.len());
    while let Some(failure) = state.subscription_failures.pop_front() {
        if failure.0 == subscription_id {
            capacity.release_event_bytes(failure_logical_bytes(&failure));
        } else {
            retained.push_back(failure);
        }
    }
    state.subscription_failures = retained;
}

/// Removes a live sender and, when requested, discards its queue atomically
/// from capacity accounting. Slow subscribers are cancelled rather than
/// allowing their detached queues to retain unbounded memory.
fn remove_subscriber(
    state: &mut EventState,
    capacity: &CapacityLedger,
    subscription_id: u64,
    discard_queued: bool,
) {
    let subscriber = state.subscribers.remove(&subscription_id);
    if discard_queued {
        if let Some(subscriber) = subscriber {
            subscriber.cancellation.cancel();
        }
        if let Some(bytes) = state.subscription_bytes.remove(&subscription_id) {
            capacity.release_event_bytes(bytes.swap(0, Ordering::Relaxed));
        }
    }
}

/// Reclaims event-owned storage until an exact reservation succeeds.
///
/// Replay is intentionally the first sacrifice. Delivery failures are next,
/// and live queues are terminated only when retained diagnostic history cannot
/// make enough room. Sessions are never touched from the event path.
fn reserve_with_reclamation(state: &mut EventState, capacity: &CapacityLedger, bytes: u64) -> bool {
    if bytes == 0 || capacity.try_reserve_event_bytes(bytes) {
        return true;
    }
    while release_oldest_retained(state, capacity) {
        state.replay_overflowed = true;
        if capacity.try_reserve_event_bytes(bytes) {
            return true;
        }
    }
    while release_oldest_failure(state, capacity) {
        if capacity.try_reserve_event_bytes(bytes) {
            return true;
        }
    }
    let subscriber_ids = state.subscribers.keys().copied().collect::<Vec<_>>();
    for subscription_id in subscriber_ids {
        remove_subscriber(state, capacity, subscription_id, true);
        if capacity.try_reserve_event_bytes(bytes) {
            return true;
        }
    }
    false
}

fn subtract_saturating(value: &AtomicU64, amount: u64) -> u64 {
    value
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_sub(amount))
        })
        .map_or(0, |previous| previous.min(amount))
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CAPACITY)
    }
}

impl Drop for EventHub {
    fn drop(&mut self) {
        let mut state = self.state.lock();
        let subscriber_ids = state.subscribers.keys().copied().collect::<Vec<_>>();
        for subscription_id in subscriber_ids {
            remove_subscriber(&mut state, self.capacity.as_ref(), subscription_id, true);
        }
        for queued in state.subscription_bytes.values() {
            self.capacity
                .release_event_bytes(queued.swap(0, Ordering::Relaxed));
        }
        state.subscription_bytes.clear();
        while release_oldest_retained(&mut state, self.capacity.as_ref()) {}
        while release_oldest_failure(&mut state, self.capacity.as_ref()) {}
        self.capacity
            .release_event_bytes(pending_capture_bytes(&state.pending_capture));
        state.pending_capture.clear();
    }
}
