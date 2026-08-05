use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    AppError, AppResult, CapacityLedger, SubscriptionAckViewModel, UiEventEnvelope, UiEventPayload,
};

use super::{
    EventReplay, TrackedEventReceiver, TrackedReplay,
    storage::{
        failure_logical_bytes, release_oldest_failure, remove_subscriber,
        reserve_event_history_bytes, snapshot_required_event, subtract_saturating,
    },
    types::{EventHub, EventState, EventSubscription, LiveSubscriber},
};

impl EventHub {
    /// 原子捕获回放边界，并注册独立实时队列。
    ///
    /// 调用方先发送 `replay`，再消费 `live`，即可保持全局事件顺序。
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

        // 克隆前为回放事件的第二份所有权预留容量。无法完整接纳时，只返回一个有界的
        // SnapshotRequired，避免短暂产生未记账副本。
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
                .insert(subscription_id, Arc::clone(&queued_logical_bytes));
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
        remove_subscriber(&mut self.state.lock(), subscription_id);
    }

    /// 返回非阻塞投递失败，供适配器记录或展示。
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
}

pub(super) fn replay_locked(state: &EventState, after_event_id: u64) -> EventReplay {
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

/// 将一个已进入全局历史的事件非阻塞分发给所有实时订阅者。
///
/// 订阅按当前已排队字节从少到多处理，让容量紧张时优先服务较健康的消费者。每次克隆事件
/// 前先向全局 `CapacityLedger` 预留逻辑字节，入队失败则立即归还，避免 `WebView` 消费变慢时
/// 产生未记账内存。缺失记账、容量不足或队列已满都视为订阅失去连续性：终止该订阅并记录
/// `SnapshotRequired`，由前端重新 bootstrap，而不是继续投递一个存在事件缺口的伪实时流。
pub(super) fn dispatch_live(
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
        terminate_live_subscription(state, capacity, envelope, subscription_id);
        while state.subscription_failures.len() > EventHub::DEFAULT_FAILURE_CAPACITY {
            release_oldest_failure(state, capacity);
        }
    }
    !overflow_was_active && state.replay_overflowed
}

fn terminate_live_subscription(
    state: &mut EventState,
    capacity: &CapacityLedger,
    envelope: &UiEventEnvelope,
    subscription_id: u64,
) {
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
}
