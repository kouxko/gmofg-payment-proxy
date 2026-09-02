use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
};

use chrono::Utc;

use crate::{CapacityLedger, CaptureRowViewModel, UiEventEnvelope, UiEventPayload};

use super::types::EventState;

pub(super) fn snapshot_required_event(current_cursor: u64) -> UiEventEnvelope {
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

pub(super) fn pending_capture_bytes(rows: &[CaptureRowViewModel]) -> u64 {
    if rows.is_empty() {
        0
    } else {
        serde_json::to_vec(rows).map_or(0, |bytes| bytes.len() as u64)
    }
}

pub(super) fn failure_logical_bytes(failure: &(u64, UiEventEnvelope)) -> u64 {
    8_u64.saturating_add(failure.1.logical_bytes())
}

pub(super) fn release_oldest_retained(state: &mut EventState, capacity: &CapacityLedger) -> bool {
    let Some(event) = state.retained.pop_front() else {
        return false;
    };
    capacity.release_event_bytes(event.logical_bytes());
    true
}

pub(super) fn release_oldest_failure(state: &mut EventState, capacity: &CapacityLedger) -> bool {
    let Some(failure) = state.subscription_failures.pop_front() else {
        return false;
    };
    capacity.release_event_bytes(failure_logical_bytes(&failure));
    true
}

pub(super) fn remove_subscription_failures(
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

/// 停止后续投递，但不假定接收器的缓冲事件已经销毁。
///
/// 队列由接收器拥有，只有其 `Drop` 可以释放排队字节。即使适配器很晚才丢弃接收器，
/// 此约束仍能保持容量记账准确。
pub(super) fn remove_subscriber(state: &mut EventState, subscription_id: u64) {
    if let Some(subscriber) = state.subscribers.remove(&subscription_id) {
        subscriber.cancellation.cancel();
    }
    state.subscription_bytes.remove(&subscription_id);
}

/// 因共享容量压力终止订阅前，先设置固定大小的控制通道。
///
/// 接收器会销毁自己的队列并恰好返回一个 `SnapshotRequired`，即使普通失败事件无法预留。
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

/// 只回收当前状态锁实际拥有的内存后，为历史事件或实时副本预留容量。
///
/// 接收器队列不在回收范围内：取消订阅不会销毁其通道，队列字节要到接收器释放时才归还。
pub(super) fn reserve_event_history_bytes(
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

/// 回收事件存储，直到精确预留成功。
///
/// 优先牺牲回放，其次是投递失败记录；只有诊断历史仍不足时才终止最慢实时队列。
pub(super) fn reserve_with_reclamation(
    state: &mut EventState,
    capacity: &CapacityLedger,
    bytes: u64,
) -> bool {
    if reserve_event_history_bytes(state, capacity, bytes) {
        return true;
    }

    // 队列字节要到接收器释放时才能归还。这里只取消一个最慢订阅，不伪造本次预留成功，
    // 也不连带终止健康订阅。
    if let Some(subscription_id) = slowest_subscription(state) {
        terminate_subscriber_for_snapshot(state, subscription_id);
    }
    false
}

/// 原子替换一份已记账事件，避免释放与重新预留之间被其他会话抢占容量。
pub(super) fn replace_event_with_reclamation(
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
    if let Some(subscription_id) = slowest_subscription(state) {
        terminate_subscriber_for_snapshot(state, subscription_id);
    }
    false
}

fn slowest_subscription(state: &EventState) -> Option<u64> {
    state
        .subscription_bytes
        .iter()
        .max_by_key(|(_, queued)| queued.load(Ordering::Relaxed))
        .map(|(subscription_id, _)| *subscription_id)
}

pub(super) fn subtract_saturating(value: &AtomicU64, amount: u64) -> u64 {
    value
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_sub(amount))
        })
        .map_or(0, |previous| previous.min(amount))
}
