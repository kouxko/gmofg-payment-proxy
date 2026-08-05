use chrono::{DateTime, Utc};

use crate::{CaptureRowViewModel, RuntimeEpoch, UiEventEnvelope, UiEventPayload};

use super::{
    retention::{append_log, record_replay_overflow_warning},
    storage::{pending_capture_bytes, reserve_with_reclamation},
    subscription::dispatch_live,
    types::{EventHub, EventState, PendingEvent},
};

impl EventHub {
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
            let _ = flush_capture_locked(
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
            // `push` 紧邻此分支，但仍保持无 panic，避免未来重构把记账偏差变成进程崩溃。
            let row = state.pending_capture.pop()?;
            if !state.pending_capture.is_empty() {
                let _ = flush_capture_locked(
                    &mut state,
                    self.replay_capacity,
                    self.capacity.as_ref(),
                    occurred_at,
                );
            }
            let (direct, warning) = append_log(
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
            let live_overflow_started = dispatch_live(&mut state, self.capacity.as_ref(), &direct);
            let warning = warning.or_else(|| {
                live_overflow_started.then(|| {
                    record_replay_overflow_warning(
                        &mut state,
                        self.replay_capacity,
                        self.capacity.as_ref(),
                        &direct,
                    )
                })
            });
            if let Some(warning) = warning {
                dispatch_live(&mut state, self.capacity.as_ref(), &warning);
            }
            return Some(direct);
        }
        (state.pending_capture.len() >= Self::CAPTURE_BATCH_SIZE)
            .then(|| {
                flush_capture_locked(
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
            flush_capture_locked(
                &mut state,
                self.replay_capacity,
                self.capacity.as_ref(),
                now,
            )
        })
        .flatten()
    }

    pub fn flush_capture(&self, now: DateTime<Utc>) -> Option<UiEventEnvelope> {
        flush_capture_locked(
            &mut self.state.lock(),
            self.replay_capacity,
            self.capacity.as_ref(),
            now,
        )
    }
}

fn flush_capture_locked(
    state: &mut EventState,
    replay_capacity: usize,
    capacity: &crate::CapacityLedger,
    occurred_at: DateTime<Utc>,
) -> Option<UiEventEnvelope> {
    if state.pending_capture.is_empty() {
        return None;
    }
    let pending_bytes = pending_capture_bytes(&state.pending_capture);
    let rows = std::mem::take(&mut state.pending_capture);
    let epoch = state.capture_epoch.take();
    state.capture_started_at = None;
    let (envelope, warning) = append_log(
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
    let live_overflow_started = dispatch_live(state, capacity, &envelope);
    let warning = warning.or_else(|| {
        live_overflow_started
            .then(|| record_replay_overflow_warning(state, replay_capacity, capacity, &envelope))
    });
    if let Some(warning) = warning {
        dispatch_live(state, capacity, &warning);
    }
    Some(envelope)
}
