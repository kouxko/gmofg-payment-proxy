//! 有序实时事件、回放与订阅生命周期。
//!
//! `EventHub` 连接事件生产者与 Tauri、未来 TUI 或测试适配器。它按游标回放事件，并
//! 对回放副本和慢订阅者占用的内存记账，防止实时展示拖垮代理核心。

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
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
/// 按游标读取历史事件的结果。
///
/// `snapshot_required` 为真表示请求位置太旧，调用方必须重新查询页面快照。
pub struct EventReplay {
    pub events: Vec<UiEventEnvelope>,
    pub current_cursor: u64,
    pub snapshot_required: bool,
}

#[derive(Debug)]
/// 一次完整订阅：确认信息、历史回放以及后续实时接收器。
pub struct EventSubscription {
    pub subscription_id: u64,
    pub ack: SubscriptionAckViewModel,
    pub replay: TrackedReplay,
    pub live: TrackedEventReceiver,
}

/// 带内存记账的历史回放集合。
///
/// 克隆事件在外层真正消费前仍占用容量；回调返回后才释放对应字节。未发送部分会在
/// `Drop` 时自动归还，避免 Tauri 序列化期间出现未记账副本。
#[derive(Debug)]
pub struct TrackedReplay {
    events: VecDeque<UiEventEnvelope>,
    reserved_logical_bytes: u64,
    capacity: Arc<CapacityLedger>,
}

impl TrackedReplay {
    fn new(events: Vec<UiEventEnvelope>, capacity: Arc<CapacityLedger>) -> Self {
        let reserved_logical_bytes = events.iter().map(UiEventEnvelope::logical_bytes).sum();
        Self {
            events: events.into(),
            reserved_logical_bytes,
            capacity,
        }
    }

    pub fn drain_with<E>(
        &mut self,
        mut consume: impl FnMut(UiEventEnvelope) -> Result<(), E>,
    ) -> Result<(), E> {
        while let Some(event) = self.events.pop_front() {
            let bytes = event.logical_bytes();
            let result = consume(event);
            self.reserved_logical_bytes = self.reserved_logical_bytes.saturating_sub(bytes);
            self.capacity.release_event_bytes(bytes);
            result?;
        }
        Ok(())
    }
}

impl Drop for TrackedReplay {
    fn drop(&mut self) {
        self.capacity
            .release_event_bytes(self.reserved_logical_bytes);
        self.reserved_logical_bytes = 0;
    }
}

#[derive(Debug)]
/// 带内存记账的实时事件接收器，取出或丢弃事件时都会归还容量。
pub struct TrackedEventReceiver {
    receiver: mpsc::Receiver<UiEventEnvelope>,
    queued_logical_bytes: Arc<AtomicU64>,
    state: Weak<Mutex<EventState>>,
    capacity: Arc<CapacityLedger>,
    subscription_id: u64,
    cancellation: CancellationToken,
    snapshot_required_on_cancel: Arc<AtomicBool>,
    terminal_event_id: Arc<AtomicU64>,
}

impl TrackedEventReceiver {
    pub async fn recv(&mut self) -> Option<UiEventEnvelope> {
        let event = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => {
                if self
                    .snapshot_required_on_cancel
                    .swap(false, Ordering::AcqRel)
                {
                    self.discard_queued_events();
                    return Some(UiEventEnvelope {
                        event_id: self.terminal_event_id.load(Ordering::Acquire),
                        runtime_epoch: None,
                        occurred_at: Utc::now(),
                        entity_id: None,
                        entity_revision: None,
                        payload: UiEventPayload::SnapshotRequired {
                            reason: "实时事件容量不足，订阅已终止；请重新获取应用快照。".into(),
                        },
                    });
                }
                return None;
            },
            event = self.receiver.recv() => event?,
        };
        let released = subtract_saturating(&self.queued_logical_bytes, event.logical_bytes());
        self.capacity.release_event_bytes(released);
        Some(event)
    }

    /// Destroys receiver-owned buffered events before releasing their logical
    /// reservations. This is used only for the bounded control termination
    /// path, where returning stale queued data before `SnapshotRequired` would
    /// violate event ordering.
    fn discard_queued_events(&mut self) {
        self.receiver.close();
        while let Ok(event) = self.receiver.try_recv() {
            let released = subtract_saturating(&self.queued_logical_bytes, event.logical_bytes());
            self.capacity.release_event_bytes(released);
        }
        let residual = self.queued_logical_bytes.swap(0, Ordering::Relaxed);
        self.capacity.release_event_bytes(residual);
    }
}

impl Drop for TrackedEventReceiver {
    fn drop(&mut self) {
        self.cancellation.cancel();
        let released = self.queued_logical_bytes.swap(0, Ordering::Relaxed);
        self.capacity.release_event_bytes(released);
        if let Some(state) = self.state.upgrade() {
            let mut state = state.lock();
            remove_subscriber(&mut state, self.subscription_id);
            remove_subscription_failures(&mut state, self.capacity.as_ref(), self.subscription_id);
        }
    }
}

#[derive(Debug)]
/// 应用内唯一的有序事件中心。
///
/// 所有事件获得单调递增 ID；抓包高频事件合批，控制事件立即发布。内部同时维护有限
/// 回放日志和每个订阅者的有界队列。
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
    snapshot_required_on_cancel: Arc<AtomicBool>,
    terminal_event_id: Arc<AtomicU64>,
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
            0,
            PendingEvent {
                runtime_epoch,
                occurred_at,
                entity_id,
                entity_revision,
                payload,
            },
        );
        let live_overflow_started =
            Self::dispatch_live(&mut state, self.capacity.as_ref(), &envelope);
        let warning = warning.or_else(|| {
            live_overflow_started.then(|| {
                Self::record_replay_overflow_warning(
                    &mut state,
                    self.replay_capacity,
                    self.capacity.as_ref(),
                    &envelope,
                )
            })
        });
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
                0,
                PendingEvent {
                    runtime_epoch: Some(runtime_epoch),
                    occurred_at,
                    entity_id: None,
                    entity_revision: None,
                    payload: UiEventPayload::CaptureRowsAdded(vec![row]),
                },
            );
            let live_overflow_started =
                Self::dispatch_live(&mut state, self.capacity.as_ref(), &direct);
            let warning = warning.or_else(|| {
                live_overflow_started.then(|| {
                    Self::record_replay_overflow_warning(
                        &mut state,
                        self.replay_capacity,
                        self.capacity.as_ref(),
                        &direct,
                    )
                })
            });
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
        let current_cursor = state.next_id.saturating_sub(1);
        let oldest = state
            .retained
            .front()
            .map_or(state.next_id, |event| event.event_id);
        let mut snapshot_required =
            after_event_id < current_cursor && after_event_id.saturating_add(1) < oldest;

        if !snapshot_required && state.subscribers.len() >= Self::MAX_SUBSCRIBERS {
            return Err(
                AppError::new("RESOURCE_EXHAUSTED", "实时事件订阅数量已达到上限。")
                    .retryable("请关闭未使用的窗口后重试。"),
            );
        }

        // Measure retained references and reserve their second ownership before
        // cloning. If the full replay cannot be admitted, return one bounded
        // SnapshotRequired event instead of briefly creating unaccounted data.
        let mut replay_events = Vec::new();
        if !snapshot_required {
            let replay_bytes = state
                .retained
                .iter()
                .filter(|event| event.event_id > after_event_id)
                .map(UiEventEnvelope::logical_bytes)
                .sum();
            if self.capacity.try_reserve_event_bytes(replay_bytes) {
                replay_events = state
                    .retained
                    .iter()
                    .filter(|event| event.event_id > after_event_id)
                    .cloned()
                    .collect();
            } else {
                snapshot_required = true;
            }
        }
        if snapshot_required {
            let snapshot = snapshot_required_event(current_cursor);
            if !reserve_event_history_bytes(
                &mut state,
                self.capacity.as_ref(),
                snapshot.logical_bytes(),
            ) {
                return Err(AppError::new(
                    "RESOURCE_EXHAUSTED",
                    "实时事件补发数据超过当前内存容量。",
                )
                .retryable("请重新获取应用快照后重试。"));
            }
            replay_events.push(snapshot);
        }

        let subscription_id = state.next_subscription_id;
        state.next_subscription_id = state.next_subscription_id.saturating_add(1);
        let (sender, receiver) =
            mpsc::channel(queue_capacity.clamp(1, Self::DEFAULT_SUBSCRIBER_CAPACITY));
        let queued_logical_bytes = Arc::new(AtomicU64::new(0));
        let cancellation = CancellationToken::new();
        let snapshot_required_on_cancel = Arc::new(AtomicBool::new(false));
        let terminal_event_id = Arc::new(AtomicU64::new(current_cursor));
        if !snapshot_required {
            state.subscribers.insert(
                subscription_id,
                LiveSubscriber {
                    sender,
                    cancellation: cancellation.clone(),
                    snapshot_required_on_cancel: Arc::clone(&snapshot_required_on_cancel),
                    terminal_event_id: Arc::clone(&terminal_event_id),
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
                current_event_id: current_cursor,
                snapshot_required,
            },
            replay: TrackedReplay::new(replay_events, Arc::clone(&self.capacity)),
            live: TrackedEventReceiver {
                receiver,
                queued_logical_bytes,
                state: Arc::downgrade(&self.state),
                capacity: Arc::clone(&self.capacity),
                subscription_id,
                cancellation,
                snapshot_required_on_cancel,
                terminal_event_id,
            },
        })
    }

    pub fn subscribe_default(&self, after_event_id: u64) -> AppResult<EventSubscription> {
        self.subscribe(after_event_id, Self::DEFAULT_SUBSCRIBER_CAPACITY)
    }

    pub fn unsubscribe(&self, subscription_id: u64) {
        let mut state = self.state.lock();
        remove_subscriber(&mut state, subscription_id);
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
            remove_subscriber(&mut state, subscription_id);
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
                events: vec![snapshot_required_event(current_cursor)],
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
        let epoch = state.capture_epoch.take();
        state.capture_started_at = None;
        let (envelope, warning) = Self::append_log(
            state,
            replay_capacity,
            capacity,
            pending_bytes,
            PendingEvent {
                runtime_epoch: epoch,
                occurred_at,
                entity_id: None,
                entity_revision: None,
                payload: UiEventPayload::CaptureRowsAdded(rows),
            },
        );
        let live_overflow_started = Self::dispatch_live(state, capacity, &envelope);
        let warning = warning.or_else(|| {
            live_overflow_started.then(|| {
                Self::record_replay_overflow_warning(state, replay_capacity, capacity, &envelope)
            })
        });
        if let Some(warning) = warning {
            Self::dispatch_live(state, capacity, &warning);
        }
        Some(envelope)
    }

    fn append_log(
        state: &mut EventState,
        replay_capacity: usize,
        capacity: &CapacityLedger,
        replaced_event_bytes: u64,
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
        let retained = if replaced_event_bytes == 0 {
            reserve_with_reclamation(state, capacity, envelope_bytes)
        } else {
            replace_event_with_reclamation(state, capacity, replaced_event_bytes, envelope_bytes)
        };
        if !retained && replaced_event_bytes > 0 {
            capacity.release_event_bytes(replaced_event_bytes);
        }
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

    /// Records the one-way transition from complete replay history to
    /// snapshot-required recovery when live-delivery admission caused the
    /// eviction.
    ///
    /// `append_log` handles the same transition while retaining the primary
    /// event. This companion covers the later live-clone reservation phase so
    /// replay truncation is never silent.
    fn record_replay_overflow_warning(
        state: &mut EventState,
        replay_capacity: usize,
        capacity: &CapacityLedger,
        cause: &UiEventEnvelope,
    ) -> UiEventEnvelope {
        let warning = UiEventEnvelope {
            event_id: state.next_id,
            runtime_epoch: cause.runtime_epoch,
            occurred_at: cause.occurred_at,
            entity_id: None,
            entity_revision: None,
            payload: UiEventPayload::ResourceWarning {
                message: "UI 补发日志已淘汰旧事件；页面必须重新查询快照。".into(),
            },
        };
        state.next_id = state.next_id.saturating_add(1);
        if reserve_event_history_bytes(state, capacity, warning.logical_bytes()) {
            state.retained.push_back(warning.clone());
        }
        while state.retained.len() > replay_capacity {
            release_oldest_retained(state, capacity);
        }
        warning
    }

    fn dispatch_live(
        state: &mut EventState,
        capacity: &CapacityLedger,
        envelope: &UiEventEnvelope,
    ) -> bool {
        let overflow_was_active = state.replay_overflowed;
        let mut terminated = Vec::new();
        let mut subscription_ids = state.subscribers.keys().copied().collect::<Vec<_>>();
        subscription_ids.sort_by_key(|subscription_id| {
            state
                .subscription_bytes
                .get(subscription_id)
                .map_or(u64::MAX, |bytes| bytes.load(Ordering::Relaxed))
        });

        for subscription_id in subscription_ids {
            let Some(subscriber) = state.subscribers.get(&subscription_id) else {
                continue;
            };
            let sender = subscriber.sender.clone();
            let Some(queued_bytes) = state.subscription_bytes.get(&subscription_id).cloned() else {
                terminated.push(subscription_id);
                continue;
            };
            let logical_bytes = envelope.logical_bytes();
            if !reserve_event_history_bytes(state, capacity, logical_bytes) {
                terminated.push(subscription_id);
                continue;
            }
            queued_bytes.fetch_add(logical_bytes, Ordering::Relaxed);
            if sender.try_send(envelope.clone()).is_err() {
                let released = subtract_saturating(&queued_bytes, logical_bytes);
                capacity.release_event_bytes(released);
                terminated.push(subscription_id);
            }
        }
        for subscription_id in terminated {
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
            if reserve_event_history_bytes(state, capacity, failure_bytes) {
                state.subscription_failures.push_back(failure);
            } else if let Some(subscriber) = state.subscribers.get(&subscription_id) {
                subscriber
                    .terminal_event_id
                    .store(envelope.event_id, Ordering::Release);
                subscriber
                    .snapshot_required_on_cancel
                    .store(true, Ordering::Release);
            }
            remove_subscriber(state, subscription_id);
            while state.subscription_failures.len() > Self::DEFAULT_FAILURE_CAPACITY {
                release_oldest_failure(state, capacity);
            }
        }
        !overflow_was_active && state.replay_overflowed
    }
}

fn snapshot_required_event(current_cursor: u64) -> UiEventEnvelope {
    UiEventEnvelope {
        event_id: current_cursor,
        runtime_epoch: None,
        occurred_at: Utc::now(),
        entity_id: None,
        entity_revision: None,
        payload: UiEventPayload::SnapshotRequired {
            reason: "事件游标已过期，请重新获取应用快照。".into(),
        },
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

/// Stops future delivery without pretending the receiver's buffered events
/// have already been destroyed.
///
/// The receiver owns the queue, so its `Drop` implementation is the only
/// authority allowed to release queued bytes. This keeps accounting exact even
/// when cancellation is observed long before the adapter drops the receiver.
fn remove_subscriber(state: &mut EventState, subscription_id: u64) {
    if let Some(subscriber) = state.subscribers.remove(&subscription_id) {
        subscriber.cancellation.cancel();
    }
    state.subscription_bytes.remove(&subscription_id);
}

/// Arms the fixed-size control lane before cancelling a subscriber because of
/// shared-capacity pressure.
///
/// The receiver will destroy its owned queue and return exactly one
/// `SnapshotRequired` event even when no ordinary event/failure envelope can
/// be reserved.
fn terminate_subscriber_for_snapshot(state: &mut EventState, subscription_id: u64) {
    if let Some(subscriber) = state.subscribers.get(&subscription_id) {
        subscriber
            .terminal_event_id
            .store(state.next_id.saturating_sub(1), Ordering::Release);
        subscriber
            .snapshot_required_on_cancel
            .store(true, Ordering::Release);
    }
    remove_subscriber(state, subscription_id);
}

/// Reserves event history or one live delivery clone after reclaiming only
/// memory whose ownership is held by this state lock.
///
/// Receiver queues are intentionally excluded: cancelling a subscriber does
/// not destroy its buffered channel, so those bytes remain owned until the
/// receiver is dropped.
fn reserve_event_history_bytes(
    state: &mut EventState,
    capacity: &CapacityLedger,
    bytes: u64,
) -> bool {
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
    false
}

/// Reclaims event-owned storage until an exact reservation succeeds.
///
/// Replay is intentionally the first sacrifice. Delivery failures are next,
/// and live queues are terminated only when retained diagnostic history cannot
/// make enough room. Sessions are never touched from the event path.
fn reserve_with_reclamation(state: &mut EventState, capacity: &CapacityLedger, bytes: u64) -> bool {
    if reserve_event_history_bytes(state, capacity, bytes) {
        return true;
    }

    // Queue bytes cannot be released until the receiver is dropped. Cancel the
    // single slowest subscriber to trigger that handoff, but do not pretend the
    // current reservation can succeed or terminate healthy subscribers.
    let slowest = state
        .subscription_bytes
        .iter()
        .max_by_key(|(_, queued)| queued.load(Ordering::Relaxed))
        .map(|(subscription_id, _)| *subscription_id);
    if let Some(subscription_id) = slowest {
        terminate_subscriber_for_snapshot(state, subscription_id);
    }
    false
}

/// Replaces one already-accounted event allocation without exposing a free
/// interval to concurrent session admission.
fn replace_event_with_reclamation(
    state: &mut EventState,
    capacity: &CapacityLedger,
    current: u64,
    replacement: u64,
) -> bool {
    if capacity.try_replace_event_bytes(current, replacement) {
        return true;
    }
    while release_oldest_retained(state, capacity) {
        state.replay_overflowed = true;
        if capacity.try_replace_event_bytes(current, replacement) {
            return true;
        }
    }
    while release_oldest_failure(state, capacity) {
        if capacity.try_replace_event_bytes(current, replacement) {
            return true;
        }
    }
    let slowest = state
        .subscription_bytes
        .iter()
        .max_by_key(|(_, queued)| queued.load(Ordering::Relaxed))
        .map(|(subscription_id, _)| *subscription_id);
    if let Some(subscription_id) = slowest {
        terminate_subscriber_for_snapshot(state, subscription_id);
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
            remove_subscriber(&mut state, subscription_id);
        }
        state.subscription_bytes.clear();
        while release_oldest_retained(&mut state, self.capacity.as_ref()) {}
        while release_oldest_failure(&mut state, self.capacity.as_ref()) {}
        self.capacity
            .release_event_bytes(pending_capture_bytes(&state.pending_capture));
        state.pending_capture.clear();
    }
}
