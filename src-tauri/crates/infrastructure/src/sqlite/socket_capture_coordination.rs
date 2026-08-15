//! Socket capture 发布快照与持久化变更之间的进程内线性化状态。

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct SocketCaptureGeneration {
    reset: u64,
    workspace: u64,
    workspace_counter: Arc<AtomicU64>,
}

/// completion event 发布期间持有的协调读许可。
///
/// 它不保护 `SQLite connection`；`clear/reset` 只用对应写锁等待旧事件结束或使其失效。
pub(crate) struct SocketCaptureCompletionPermit<'a> {
    _guard: RwLockReadGuard<'a, ()>,
}

#[derive(Debug, Default)]
pub(super) struct SocketCaptureCoordination {
    pub(super) mutation_gate: Mutex<()>,
    pub(super) completion_gate: RwLock<()>,
    reset_generation: AtomicU64,
    workspace_generations: RwLock<BTreeMap<Uuid, Arc<AtomicU64>>>,
}

impl SocketCaptureCoordination {
    pub(super) fn snapshot(&self, workspace_id: Uuid) -> SocketCaptureGeneration {
        let workspace_counter = self.workspace_counter(workspace_id);
        SocketCaptureGeneration {
            reset: self.reset_generation.load(Ordering::Acquire),
            workspace: workspace_counter.load(Ordering::Acquire),
            workspace_counter,
        }
    }

    pub(super) fn is_current(&self, generation: &SocketCaptureGeneration) -> bool {
        self.reset_generation.load(Ordering::Acquire) == generation.reset
            && generation.workspace_counter.load(Ordering::Acquire) == generation.workspace
    }

    pub(super) fn completion_if_current(
        &self,
        generation: &SocketCaptureGeneration,
    ) -> Option<SocketCaptureCompletionPermit<'_>> {
        let guard = self.completion_gate.read();
        self.is_current(generation)
            .then_some(SocketCaptureCompletionPermit { _guard: guard })
    }

    pub(super) fn bump_workspace(&self, workspace_id: Uuid) -> Result<(), &'static str> {
        increment(
            &self.workspace_counter(workspace_id),
            "Workspace capture generation 耗尽",
        )
    }

    pub(super) fn bump_reset(&self) -> Result<(), &'static str> {
        increment(&self.reset_generation, "capture reset generation 耗尽")
    }

    fn workspace_counter(&self, workspace_id: Uuid) -> Arc<AtomicU64> {
        if let Some(counter) = self.workspace_generations.read().get(&workspace_id) {
            return Arc::clone(counter);
        }
        Arc::clone(
            self.workspace_generations
                .write()
                .entry(workspace_id)
                .or_insert_with(|| Arc::new(AtomicU64::new(0))),
        )
    }
}

fn increment(counter: &AtomicU64, exhausted: &'static str) -> Result<(), &'static str> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .map(|_| ())
        .map_err(|_| exhausted)
}
