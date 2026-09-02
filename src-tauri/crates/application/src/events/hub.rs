use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{CapacityLedger, Revision, RuntimeEpoch, UiEventEnvelope, UiEventPayload};

use super::{
    EventReplay,
    retention::{append_log, record_replay_overflow_warning},
    storage::{
        pending_capture_bytes, release_oldest_failure, release_oldest_retained, remove_subscriber,
    },
    subscription::{dispatch_live, replay_locked},
    types::{EventHub, EventState, PendingEvent},
};

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

    /// 使用进程级统一容量账本创建事件中心。
    ///
    /// `replay_capacity` 只限制事件条数；回放、待合批事件、实时队列和失败记录占用的
    /// 字节都由 `capacity` 决定是否接纳。
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
        let (envelope, warning) = append_log(
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
        let live_overflow_started = dispatch_live(&mut state, self.capacity.as_ref(), &envelope);
        let warning = warning.or_else(|| {
            live_overflow_started.then(|| {
                record_replay_overflow_warning(
                    &mut state,
                    self.replay_capacity,
                    self.capacity.as_ref(),
                    &envelope,
                )
            })
        });
        if let Some(warning) = warning {
            dispatch_live(&mut state, self.capacity.as_ref(), &warning);
        }
        envelope
    }

    pub fn replay_after(&self, after_event_id: u64) -> EventReplay {
        replay_locked(&self.state.lock(), after_event_id)
    }

    pub fn current_cursor(&self) -> u64 {
        self.state.lock().next_id.saturating_sub(1)
    }

    pub fn logical_bytes(&self) -> u64 {
        self.capacity.event_bytes()
    }

    /// 在降低共享容量上限前，回收可丢弃的事件存储。
    ///
    /// 待合批抓包仍会保留；调用方随后可让会话存储原子应用新上限，只淘汰已完成会话。
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

    /// 启动生产环境 100 毫秒抓包刷新时钟，不让网络路径等待 UI 订阅者。
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
