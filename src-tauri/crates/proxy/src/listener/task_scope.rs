use std::{
    collections::BTreeMap,
    fmt,
    future::Future,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex, MutexGuard},
};

use futures_util::FutureExt;
use tokio::sync::Notify;
use tokio::task::AbortHandle;
use tokio_util::task::TaskTracker;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ChildTaskId(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildTaskError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

impl ChildTaskError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ChildTaskAggregate {
    pub(crate) panic_seen: bool,
    pub(crate) lowest_error: Option<(ChildTaskId, ChildTaskError)>,
    pub(crate) completed_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopePhase {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskScopeSnapshot {
    pub(crate) phase: ScopePhase,
    pub(crate) live_count: usize,
    pub(crate) aggregate: ChildTaskAggregate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpawnRejected;

impl fmt::Display for SpawnRejected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("connection task scope is closed")
    }
}

impl std::error::Error for SpawnRejected {}

#[derive(Debug, Clone)]
pub(crate) struct ConnectionTaskScope {
    tracker: TaskTracker,
    state: Arc<Mutex<ScopeState>>,
    fatal: Arc<Notify>,
}

#[derive(Debug)]
struct ScopeState {
    phase: ScopePhase,
    next_id: u64,
    live: BTreeMap<ChildTaskId, AbortHandle>,
    aggregate: ChildTaskAggregate,
}

impl ConnectionTaskScope {
    pub(crate) fn new() -> Self {
        Self {
            tracker: TaskTracker::new(),
            fatal: Arc::new(Notify::new()),
            state: Arc::new(Mutex::new(ScopeState {
                phase: ScopePhase::Open,
                next_id: 0,
                live: BTreeMap::new(),
                aggregate: ChildTaskAggregate::default(),
            })),
        }
    }

    pub(crate) fn spawn_owned<F>(&self, future: F) -> Result<ChildTaskId, SpawnRejected>
    where
        F: Future<Output = Result<(), ChildTaskError>> + Send + 'static,
    {
        let mut state = lock(&self.state);
        if state.phase == ScopePhase::Closed {
            return Err(SpawnRejected);
        }

        let id = ChildTaskId(state.next_id);
        state.next_id = state
            .next_id
            .checked_add(1)
            .expect("connection child task id exhausted");

        let completion = CompletionGuard::new(id, Arc::clone(&self.state), Arc::clone(&self.fatal));
        let task = async move {
            let outcome = match AssertUnwindSafe(future).catch_unwind().await {
                Ok(Ok(())) => ChildCompletion::Success,
                Ok(Err(error)) => ChildCompletion::Error(error),
                Err(_) => ChildCompletion::Panic,
            };
            completion.complete(outcome);
        };
        let join = self.tracker.spawn(task);
        state.live.insert(id, join.abort_handle());
        drop(join);
        Ok(id)
    }

    pub(crate) fn close(&self) {
        let mut state = lock(&self.state);
        if state.phase == ScopePhase::Open {
            state.phase = ScopePhase::Closed;
            self.tracker.close();
        }
    }

    pub(crate) async fn drain(&self) {
        self.tracker.wait().await;
    }

    pub(crate) async fn fatal_notified(&self) {
        self.fatal.notified().await;
    }

    /// 测试辅助：关闭注册并等待全部子任务，用于一次性断言最终聚合结果。
    #[cfg(test)]
    pub(crate) async fn close_and_drain(&self) -> ChildTaskAggregate {
        self.close();
        self.drain().await;
        self.snapshot().aggregate
    }

    pub(crate) fn abort_live(&self) -> Vec<ChildTaskId> {
        let live = {
            let state = lock(&self.state);
            state
                .live
                .iter()
                .map(|(&id, handle)| (id, handle.clone()))
                .collect::<Vec<_>>()
        };
        for (_, handle) in &live {
            handle.abort();
        }
        live.into_iter().map(|(id, _)| id).collect()
    }

    pub(crate) fn snapshot(&self) -> TaskScopeSnapshot {
        let state = lock(&self.state);
        TaskScopeSnapshot {
            phase: state.phase,
            live_count: state.live.len(),
            aggregate: state.aggregate.clone(),
        }
    }
}

impl Default for ConnectionTaskScope {
    fn default() -> Self {
        Self::new()
    }
}

enum ChildCompletion {
    Success,
    Error(ChildTaskError),
    Panic,
}

struct CompletionGuard {
    id: ChildTaskId,
    state: Arc<Mutex<ScopeState>>,
    fatal: Arc<Notify>,
    completed: bool,
}

impl CompletionGuard {
    fn new(id: ChildTaskId, state: Arc<Mutex<ScopeState>>, fatal: Arc<Notify>) -> Self {
        Self {
            id,
            state,
            fatal,
            completed: false,
        }
    }

    fn complete(mut self, completion: ChildCompletion) {
        let fatal = matches!(completion, ChildCompletion::Panic);
        {
            let mut state = lock(&self.state);
            state.live.remove(&self.id);
            state.aggregate.completed_count = state
                .aggregate
                .completed_count
                .checked_add(1)
                .expect("connection child completion count exhausted");
            match completion {
                ChildCompletion::Success => {}
                ChildCompletion::Error(error) => {
                    let replace = state
                        .aggregate
                        .lowest_error
                        .as_ref()
                        .is_none_or(|(lowest_id, _)| self.id < *lowest_id);
                    if replace {
                        state.aggregate.lowest_error = Some((self.id, error));
                    }
                }
                ChildCompletion::Panic => state.aggregate.panic_seen = true,
            }
        }
        if fatal {
            self.fatal.notify_one();
        }
        self.completed = true;
    }
}

impl Drop for CompletionGuard {
    fn drop(&mut self) {
        if !self.completed {
            lock(&self.state).live.remove(&self.id);
        }
    }
}

fn lock(state: &Mutex<ScopeState>) -> MutexGuard<'_, ScopeState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
