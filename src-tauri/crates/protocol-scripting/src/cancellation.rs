//! 可在同一协议方向的 Frame/Decode/Encode/Display 间共享的执行取消控制。

use std::{
    fmt,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
};

const CANCELLED_BIT: u64 = 1;

/// 可克隆、线程安全的协议执行取消句柄。
///
/// 状态同时保存 generation 和取消位。`cancel` 或 `reset` 都会让已经开始的旧 generation 失效，
/// 因此取消后立即 reset 也不会把仍在运行的旧 Rhai 调用重新放行；reset 只允许之后开始的新调用。
#[derive(Clone, Default)]
pub struct ProtocolExecutionCancellation {
    state: Arc<AtomicU64>,
}

impl ProtocolExecutionCancellation {
    /// 创建未取消的初始 generation。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 取消当前 generation；重复调用保持幂等。
    pub fn cancel(&self) {
        self.state.fetch_or(CANCELLED_BIT, Ordering::AcqRel);
    }

    /// 开启一个新的未取消 generation。
    ///
    /// 正在执行的旧调用会观察到 generation 变化并终止，不会被本次 reset 复活。
    pub fn reset(&self) {
        let _ = self
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |state| {
                Some(if state & CANCELLED_BIT == 0 {
                    state.wrapping_add(2)
                } else {
                    state.wrapping_add(1)
                })
            });
    }

    /// 返回当前 generation 是否已取消。
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) & CANCELLED_BIT != 0
    }

    pub(crate) fn snapshot(&self) -> ProtocolCancellationSnapshot {
        ProtocolCancellationSnapshot(self.state.load(Ordering::Acquire))
    }

    pub(crate) fn changed_since(&self, snapshot: ProtocolCancellationSnapshot) -> bool {
        self.state.load(Ordering::Acquire) != snapshot.0
    }
}

impl fmt::Debug for ProtocolExecutionCancellation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProtocolExecutionCancellation")
            .field("cancelled", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ProtocolCancellationSnapshot(pub(crate) u64);

impl ProtocolCancellationSnapshot {
    pub(crate) const fn is_cancelled(self) -> bool {
        self.0 & CANCELLED_BIT != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_starts_new_generation_without_reviving_old_snapshot() {
        let cancellation = ProtocolExecutionCancellation::new();
        let old = cancellation.snapshot();
        cancellation.cancel();
        assert!(cancellation.is_cancelled());
        cancellation.reset();

        assert!(!cancellation.is_cancelled());
        assert!(cancellation.changed_since(old));
        assert!(!format!("{cancellation:?}").contains("generation"));
    }
}
