//! Frame Rhai 入口独占的单调时钟截止控制。

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rhai::{Dynamic, Engine};

use crate::{ProtocolExecutionCancellation, cancellation::ProtocolCancellationSnapshot};

/// Frame Engine 的 deadline 不与 Decode/Encode Engine 共享，避免两个方向或两个入口互相解除截止时间。
pub(super) struct FrameCallDeadline {
    anchor: Instant,
    deadline_ns: Arc<AtomicU64>,
    armed: Arc<AtomicBool>,
    expected_cancellation_state: Arc<AtomicU64>,
    cancellation: ProtocolExecutionCancellation,
}

impl FrameCallDeadline {
    pub(super) fn install(
        engine: &mut Engine,
        cancellation: ProtocolExecutionCancellation,
    ) -> Self {
        let anchor = Instant::now();
        let deadline_ns = Arc::new(AtomicU64::new(0));
        let armed = Arc::new(AtomicBool::new(false));
        let expected_cancellation_state = Arc::new(AtomicU64::new(0));
        let callback_deadline = Arc::clone(&deadline_ns);
        let callback_armed = Arc::clone(&armed);
        let callback_expected = Arc::clone(&expected_cancellation_state);
        let callback_cancellation = cancellation.clone();
        engine.on_progress(move |_| {
            if !callback_armed.load(Ordering::Acquire) {
                return None;
            }
            let snapshot = ProtocolCancellationSnapshot(callback_expected.load(Ordering::Acquire));
            if callback_cancellation.changed_since(snapshot) {
                return Some(Dynamic::UNIT);
            }
            let deadline = callback_deadline.load(Ordering::Relaxed);
            if deadline != 0 && elapsed_ns(anchor) >= deadline {
                // 固定 Unit 只负责中断解释器，公开错误不会包含动态终止文本。
                Some(Dynamic::UNIT)
            } else {
                None
            }
        });
        Self {
            anchor,
            deadline_ns,
            armed,
            expected_cancellation_state,
            cancellation,
        }
    }

    pub(super) fn arm(&self, duration: Duration) -> Result<Instant, ()> {
        let snapshot = self.cancellation.snapshot();
        if snapshot.is_cancelled() {
            return Err(());
        }
        let started = Instant::now();
        let deadline = elapsed_ns(self.anchor).saturating_add(duration_ns(duration));
        self.expected_cancellation_state
            .store(snapshot.0, Ordering::Release);
        // 0 专门表示未武装；运行时限制已经保证 duration 至少为 1ms。
        self.deadline_ns.store(deadline.max(1), Ordering::Relaxed);
        self.armed.store(true, Ordering::Release);
        if self.cancellation.changed_since(snapshot) {
            self.disarm();
            Err(())
        } else {
            Ok(started)
        }
    }

    pub(super) fn was_cancelled(&self) -> bool {
        self.armed.load(Ordering::Acquire)
            && self
                .cancellation
                .changed_since(ProtocolCancellationSnapshot(
                    self.expected_cancellation_state.load(Ordering::Acquire),
                ))
    }

    pub(super) fn disarm(&self) {
        self.armed.store(false, Ordering::Release);
        self.deadline_ns.store(0, Ordering::Relaxed);
    }
}

fn elapsed_ns(anchor: Instant) -> u64 {
    u64::try_from(anchor.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
