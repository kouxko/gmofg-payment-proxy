use crate::{CapacityLedger, UiEventEnvelope, UiEventPayload};

use super::{
    storage::{
        release_oldest_retained, replace_event_with_reclamation, reserve_event_history_bytes,
        reserve_with_reclamation,
    },
    types::{EventState, PendingEvent},
};

pub(super) fn append_log(
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
    let warning = overflow_started
        .then(|| append_capacity_warning(state, replay_capacity, capacity, &envelope));
    (envelope, warning)
}

fn append_capacity_warning(
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
            message: "UI 补发日志已淘汰旧事件；过期订阅需要重新获取快照。".into(),
        },
    };
    state.next_id = state.next_id.saturating_add(1);
    if reserve_with_reclamation(state, capacity, warning.logical_bytes()) {
        state.retained.push_back(warning.clone());
    }
    while state.retained.len() > replay_capacity {
        release_oldest_retained(state, capacity);
    }
    warning
}

/// 当实时投递预留引起回放淘汰时，记录从完整回放到必须刷新快照的单向转换。
///
/// `append_log` 处理保留主事件时发生的同类转换；此函数覆盖之后的实时副本预留阶段，
/// 保证回放截断不会静默发生。
pub(super) fn record_replay_overflow_warning(
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
