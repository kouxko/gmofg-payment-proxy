use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    CapacityLedger, CaptureRowViewModel, Revision, RuntimeEpoch, SubscriptionAckViewModel,
    UiEventEnvelope, UiEventPayload,
};

use super::{TrackedEventReceiver, TrackedReplay};

#[derive(Debug)]
/// 一次完整订阅：确认信息、历史回放以及后续实时接收器。
pub struct EventSubscription {
    pub subscription_id: u64,
    pub ack: SubscriptionAckViewModel,
    pub replay: TrackedReplay,
    pub live: TrackedEventReceiver,
}

#[derive(Debug)]
/// 应用内唯一的有序事件中心。
///
/// 所有事件获得单调递增 ID；抓包高频事件合批，控制事件立即发布。内部同时维护有限
/// 回放日志和每个订阅者的有界队列。
pub struct EventHub {
    pub(super) replay_capacity: usize,
    pub(super) capacity: Arc<CapacityLedger>,
    pub(super) state: Arc<Mutex<EventState>>,
}

#[derive(Debug)]
pub(super) struct EventState {
    pub(super) next_id: u64,
    pub(super) retained: VecDeque<UiEventEnvelope>,
    pub(super) pending_capture: Vec<CaptureRowViewModel>,
    pub(super) capture_epoch: Option<RuntimeEpoch>,
    pub(super) capture_started_at: Option<DateTime<Utc>>,
    pub(super) replay_overflowed: bool,
    pub(super) next_subscription_id: u64,
    pub(super) subscribers: HashMap<u64, LiveSubscriber>,
    pub(super) subscription_bytes: HashMap<u64, Arc<AtomicU64>>,
    pub(super) subscription_failures: VecDeque<(u64, UiEventEnvelope)>,
}

#[derive(Debug)]
pub(super) struct LiveSubscriber {
    pub(super) sender: mpsc::Sender<UiEventEnvelope>,
    pub(super) cancellation: CancellationToken,
    pub(super) snapshot_required_on_cancel: Arc<AtomicBool>,
    pub(super) terminal_event_id: Arc<AtomicU64>,
}

pub(super) struct PendingEvent {
    pub(super) runtime_epoch: Option<RuntimeEpoch>,
    pub(super) occurred_at: DateTime<Utc>,
    pub(super) entity_id: Option<String>,
    pub(super) entity_revision: Option<Revision>,
    pub(super) payload: UiEventPayload,
}
