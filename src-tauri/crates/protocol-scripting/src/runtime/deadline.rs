use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rhai::{Dynamic, Engine};

use crate::{ProtocolExecutionCancellation, cancellation::ProtocolCancellationSnapshot};

/// 每个 Engine 独占的单调时钟截止控制器。
///
/// 回调不能捕获某次调用的 `Instant` 引用，因此用 Engine 创建时的锚点加原子纳秒数表达当前截止
/// 时间。入口开始前重新武装，结束后解除；回调不加锁，也不共享跨 Engine 的全局状态。
pub(super) struct CallDeadline {
    anchor: Instant,
    deadline_ns: Arc<AtomicU64>,
    armed: Arc<AtomicBool>,
    expected_cancellation_state: Arc<AtomicU64>,
    cancellation: ProtocolExecutionCancellation,
}

impl CallDeadline {
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
                // 终止载荷固定为 Unit，不把取消来源、脚本状态或输入数据带入 Rhai 错误文本。
                return Some(Dynamic::UNIT);
            }
            let deadline = callback_deadline.load(Ordering::Relaxed);
            if deadline != 0 && elapsed_ns(anchor) >= deadline {
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
        // 0 保留为“未武装”；即使锚点极早，允许的 duration 也至少是 1ms。
        self.deadline_ns.store(deadline.max(1), Ordering::Relaxed);
        self.armed.store(true, Ordering::Release);
        // snapshot 读取与武装之间发生的 cancel/reset 会让当前状态不同，调用开始前直接失败。
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
