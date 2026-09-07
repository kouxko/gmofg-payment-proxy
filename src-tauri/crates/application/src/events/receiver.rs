use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use chrono::Utc;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{CapacityLedger, UiEventEnvelope, UiEventPayload};

use super::{
    storage::{remove_subscriber, remove_subscription_failures, subtract_saturating},
    types::EventState,
};

#[derive(Debug)]
/// 带内存记账的实时事件接收器，取出或丢弃事件时都会归还容量。
pub struct TrackedEventReceiver {
    pub(super) receiver: mpsc::Receiver<UiEventEnvelope>,
    pub(super) queued_logical_bytes: Arc<AtomicU64>,
    pub(super) state: Weak<Mutex<EventState>>,
    pub(super) capacity: Arc<CapacityLedger>,
    pub(super) subscription_id: u64,
    pub(super) cancellation: CancellationToken,
    pub(super) snapshot_required_on_cancel: Arc<AtomicBool>,
    pub(super) terminal_event_id: Arc<AtomicU64>,
}

impl TrackedEventReceiver {
    pub async fn recv(&mut self) -> Option<UiEventEnvelope> {
        let event = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => {
                if self.snapshot_required_on_cancel.swap(false, Ordering::AcqRel) {
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

    /// 销毁接收器拥有的缓冲事件，再释放其逻辑容量。
    ///
    /// 仅用于有界控制终止路径；若在 `SnapshotRequired` 之前返回旧事件，会破坏事件顺序。
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
