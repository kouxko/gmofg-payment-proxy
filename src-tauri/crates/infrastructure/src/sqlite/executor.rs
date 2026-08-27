//! 将同步 `rusqlite` 工作隔离到 Tokio 阻塞线程池。
//!
//! 丢弃调用方 future 只会停止等待；已经开始的阻塞闭包仍会运行到结束，并依靠
//! `SqliteStore` 的锁守卫释放连接。阻塞线程 panic 或任务无法完成时统一映射为稳定的
//! 数据库操作错误类别，闭包返回的原始数据库错误则原样保留。

use std::{path::PathBuf, sync::Arc};

use super::SqliteStore;
use crate::InfrastructureError;

#[derive(Clone, Debug)]
pub struct SqliteExecutor {
    store: Arc<SqliteStore>,
    gate: Arc<tokio::sync::Semaphore>,
}

impl SqliteExecutor {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self {
            gate: Arc::clone(&store.blocking_gate),
            store,
        }
    }

    pub async fn execute<T, E, F>(&self, operation: F) -> Result<T, E>
    where
        T: Send + 'static,
        E: From<InfrastructureError> + Send + 'static,
        F: FnOnce(&SqliteStore) -> Result<T, E> + Send + 'static,
    {
        let permit = Arc::clone(&self.gate)
            .acquire_owned()
            .await
            .map_err(|_| E::from(InfrastructureError::DatabaseExecutorUnavailable))?;
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            operation(&store)
        })
        .await
        .map_err(|source| E::from(InfrastructureError::DatabaseExecutorTerminated { source }))?
    }
}

/// Opens and initializes `SQLite` away from `Tokio` worker threads.
///
/// Dropping the caller future only stops waiting. Once the blocking task has started, `Tokio` owns
/// it until schema initialization finishes; the resulting store is then dropped if nobody awaits
/// it. Successful callers receive an executor and the exact same shared store instance.
pub async fn open_sqlite_persistence(
    path: PathBuf,
) -> Result<(SqliteExecutor, Arc<SqliteStore>), InfrastructureError> {
    open_sqlite_persistence_with(move || SqliteStore::open(&path)).await
}

async fn open_sqlite_persistence_with<F>(
    open: F,
) -> Result<(SqliteExecutor, Arc<SqliteStore>), InfrastructureError>
where
    F: FnOnce() -> Result<SqliteStore, InfrastructureError> + Send + 'static,
{
    let store = tokio::task::spawn_blocking(open)
        .await
        .map_err(|source| InfrastructureError::DatabaseExecutorTerminated { source })??;
    let store = Arc::new(store);
    Ok((SqliteExecutor::new(Arc::clone(&store)), store))
}

pub trait IntoSqlitePersistence {
    fn into_sqlite_persistence(self) -> (SqliteExecutor, Arc<SqliteStore>);
}

impl IntoSqlitePersistence for Arc<SqliteStore> {
    fn into_sqlite_persistence(self) -> (SqliteExecutor, Arc<SqliteStore>) {
        (SqliteExecutor::new(Arc::clone(&self)), self)
    }
}

impl IntoSqlitePersistence for (SqliteExecutor, Arc<SqliteStore>) {
    fn into_sqlite_persistence(self) -> (SqliteExecutor, Arc<SqliteStore>) {
        self
    }
}

impl From<Arc<SqliteStore>> for SqliteExecutor {
    fn from(store: Arc<SqliteStore>) -> Self {
        Self::new(store)
    }
}

#[cfg(test)]
mod tests {
    use std::{future::Future, pin::Pin, sync::mpsc, task::Poll};

    use tokio::sync::oneshot;

    use super::*;
    use crate::InfrastructureErrorCode;

    async fn poll_once<F>(future: &mut F) -> Poll<F::Output>
    where
        F: Future + Unpin,
    {
        std::future::poll_fn(|context| Poll::Ready(Pin::new(&mut *future).poll(context))).await
    }

    #[tokio::test(flavor = "current_thread")]
    async fn async_runtime_progresses_while_sqlite_connection_is_held_by_blocking_work() {
        let executor = SqliteExecutor::new(Arc::new(SqliteStore::in_memory().expect("store")));
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let slow_database_work = tokio::spawn({
            let executor = executor.clone();
            async move {
                executor
                    .execute(move |store| {
                        let _connection = store.connection.lock();
                        entered_tx.send(()).expect("test still waits for entry");
                        release_rx.recv().expect("test releases database work");
                        Ok::<_, InfrastructureError>(())
                    })
                    .await
            }
        });

        entered_rx.await.expect("database work entered");
        let (progress_tx, progress_rx) = oneshot::channel();
        tokio::spawn(async move {
            progress_tx.send(()).expect("test still waits for progress");
        });
        progress_rx
            .await
            .expect("current-thread runtime progressed");

        release_tx.send(()).expect("release database work");
        slow_database_work
            .await
            .expect("database task joined")
            .expect("database work succeeded");
    }

    #[tokio::test]
    async fn cancelling_waiter_does_not_strand_the_sqlite_connection_lock() {
        let executor = SqliteExecutor::new(Arc::new(SqliteStore::in_memory().expect("store")));
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = oneshot::channel();

        let waiter = tokio::spawn({
            let executor = executor.clone();
            async move {
                executor
                    .execute(move |store| {
                        let _connection = store.connection.lock();
                        entered_tx.send(()).expect("test still waits for entry");
                        release_rx.recv().expect("test releases database work");
                        finished_tx
                            .send(())
                            .expect("test still waits for blocking completion");
                        Ok::<_, InfrastructureError>(())
                    })
                    .await
            }
        });

        entered_rx.await.expect("database work entered");
        waiter.abort();
        assert!(
            waiter
                .await
                .expect_err("waiter is cancelled")
                .is_cancelled()
        );
        release_tx.send(()).expect("release database work");
        finished_rx
            .await
            .expect("blocking work completed after cancel");

        executor
            .execute(|store| {
                let _connection = store.connection.lock();
                Ok::<_, InfrastructureError>(())
            })
            .await
            .expect("later database work acquires the released lock");
    }

    #[tokio::test]
    async fn shared_gate_allows_only_one_closure_to_enter_at_a_time() {
        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        let first_executor = SqliteExecutor::new(Arc::clone(&store));
        let second_executor = SqliteExecutor::new(store);
        let (first_entered_tx, first_entered_rx) = oneshot::channel();
        let (first_release_tx, first_release_rx) = mpsc::channel();
        let first = tokio::spawn(async move {
            first_executor
                .execute(move |_| {
                    first_entered_tx.send(()).expect("first entry observed");
                    first_release_rx.recv().expect("first released");
                    Ok::<_, InfrastructureError>(())
                })
                .await
        });
        first_entered_rx.await.expect("first closure entered");

        let (second_started_tx, second_started_rx) = oneshot::channel();
        let (second_entered_tx, second_entered_rx) = oneshot::channel();
        let mut second = tokio::spawn(async move {
            second_started_tx.send(()).expect("second start observed");
            second_executor
                .execute(move |_| {
                    second_entered_tx.send(()).expect("second entry observed");
                    Ok::<_, InfrastructureError>(())
                })
                .await
        });
        second_started_rx.await.expect("second future started");
        assert!(matches!(poll_once(&mut second).await, Poll::Pending));

        first_release_tx.send(()).expect("release first closure");
        first.await.expect("first joined").expect("first succeeded");
        second_entered_rx
            .await
            .expect("second closure entered later");
        second
            .await
            .expect("second joined")
            .expect("second succeeded");
    }

    #[tokio::test]
    async fn cancelling_a_queued_future_prevents_its_closure_from_running() {
        let executor = SqliteExecutor::new(Arc::new(SqliteStore::in_memory().expect("store")));
        let (first_entered_tx, first_entered_rx) = oneshot::channel();
        let (first_release_tx, first_release_rx) = mpsc::channel();
        let first = tokio::spawn({
            let executor = executor.clone();
            async move {
                executor
                    .execute(move |_| {
                        first_entered_tx.send(()).expect("first entry observed");
                        first_release_rx.recv().expect("first released");
                        Ok::<_, InfrastructureError>(())
                    })
                    .await
            }
        });
        first_entered_rx.await.expect("first closure entered");

        let (queued_tx, queued_rx) = oneshot::channel();
        let (forbidden_tx, forbidden_rx) = oneshot::channel();
        let mut queued = tokio::spawn({
            let executor = executor.clone();
            async move {
                queued_tx.send(()).expect("queue point observed");
                executor
                    .execute(move |_| {
                        let _ = forbidden_tx.send(());
                        Ok::<_, InfrastructureError>(())
                    })
                    .await
            }
        });
        queued_rx.await.expect("queued future reached executor");
        assert!(matches!(poll_once(&mut queued).await, Poll::Pending));
        queued.abort();
        assert!(
            queued
                .await
                .expect_err("queued future cancelled")
                .is_cancelled()
        );

        first_release_tx.send(()).expect("release first closure");
        first.await.expect("first joined").expect("first succeeded");
        assert!(
            forbidden_rx.await.is_err(),
            "queued closure unexpectedly ran"
        );
    }

    #[tokio::test]
    async fn database_errors_keep_their_code_and_panics_map_to_a_stable_code() {
        let executor = SqliteExecutor::new(Arc::new(SqliteStore::in_memory().expect("store")));

        let database = executor
            .execute(|_| Err::<(), _>(InfrastructureError::RevisionConflict))
            .await
            .expect_err("database error");
        assert_eq!(database.code(), InfrastructureErrorCode::RevisionConflict);

        let terminated = executor
            .execute(|_| -> Result<(), InfrastructureError> { panic!("test panic") })
            .await
            .expect_err("panic must map to infrastructure error");
        assert_eq!(
            terminated.code(),
            InfrastructureErrorCode::DatabaseWriteFailed
        );
        assert!(matches!(
            terminated,
            InfrastructureError::DatabaseExecutorTerminated { .. }
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sqlite_open_bootstrap_does_not_block_runtime_progress() {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let opening = tokio::spawn(async move {
            open_sqlite_persistence_with(move || {
                entered_tx.send(()).expect("bootstrap entry observed");
                release_rx.recv().expect("bootstrap released");
                SqliteStore::in_memory()
            })
            .await
        });

        entered_rx.await.expect("bootstrap started");
        tokio::task::yield_now().await;
        let (progress_tx, progress_rx) = oneshot::channel();
        tokio::spawn(async move {
            progress_tx.send(()).expect("progress receiver alive");
        });
        progress_rx.await.expect("runtime progressed");

        release_tx.send(()).expect("release bootstrap");
        let (executor, store) = opening
            .await
            .expect("bootstrap task joined")
            .expect("bootstrap succeeded");
        executor
            .execute(move |store_from_executor| {
                assert!(std::ptr::eq(store_from_executor, Arc::as_ptr(&store)));
                Ok::<_, InfrastructureError>(())
            })
            .await
            .expect("executor shares opened store");
    }

    #[tokio::test]
    async fn cancelling_started_sqlite_open_only_stops_waiting() {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = oneshot::channel();
        let opening = tokio::spawn(async move {
            open_sqlite_persistence_with(move || {
                entered_tx.send(()).expect("bootstrap entry observed");
                release_rx.recv().expect("bootstrap released");
                let store = SqliteStore::in_memory()?;
                finished_tx.send(()).expect("completion observed");
                Ok(store)
            })
            .await
        });

        entered_rx.await.expect("bootstrap started");
        opening.abort();
        assert!(opening.await.expect_err("caller cancelled").is_cancelled());
        release_tx.send(()).expect("release bootstrap");
        finished_rx
            .await
            .expect("owned blocking bootstrap completed after cancellation");
    }
}
