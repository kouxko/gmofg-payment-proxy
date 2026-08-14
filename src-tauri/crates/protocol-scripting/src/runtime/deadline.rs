use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use rhai::{Dynamic, Engine};

/// 每个 Engine 独占的单调时钟截止控制器。
///
/// 回调不能捕获某次调用的 `Instant` 引用，因此用 Engine 创建时的锚点加原子纳秒数表达当前截止
/// 时间。入口开始前重新武装，结束后解除；回调不加锁，也不共享跨 Engine 的全局状态。
pub(super) struct CallDeadline {
    anchor: Instant,
    deadline_ns: Arc<AtomicU64>,
}

impl CallDeadline {
    pub(super) fn install(engine: &mut Engine) -> Self {
        let anchor = Instant::now();
        let deadline_ns = Arc::new(AtomicU64::new(0));
        let callback_deadline = Arc::clone(&deadline_ns);
        engine.on_progress(move |_| {
            let deadline = callback_deadline.load(Ordering::Relaxed);
            if deadline != 0 && elapsed_ns(anchor) >= deadline {
                // token 永不跨公开错误边界，只用于让映射器区分宿主终止。
                Some(Dynamic::from("protocol_wall_time"))
            } else {
                None
            }
        });
        Self {
            anchor,
            deadline_ns,
        }
    }

    pub(super) fn arm(&self, duration: Duration) -> Instant {
        let started = Instant::now();
        let deadline = elapsed_ns(self.anchor).saturating_add(duration_ns(duration));
        // 0 保留为“未武装”；即使锚点极早，允许的 duration 也至少是 1ms。
        self.deadline_ns.store(deadline.max(1), Ordering::Relaxed);
        started
    }

    pub(super) fn disarm(&self) {
        self.deadline_ns.store(0, Ordering::Relaxed);
    }
}

fn elapsed_ns(anchor: Instant) -> u64 {
    u64::try_from(anchor.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
