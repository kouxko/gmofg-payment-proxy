use std::{future::Future, pin::Pin, sync::mpsc, task::Poll};

use tokio::sync::oneshot;

use super::*;

async fn poll_once<F>(future: Pin<&mut F>) -> Poll<F::Output>
where
    F: Future,
{
    let mut future = future;
    std::future::poll_fn(|context| Poll::Ready(future.as_mut().poll(context))).await
}

#[tokio::test(flavor = "current_thread")]
async fn listener_snapshot_waits_asynchronously_and_queued_cancel_does_not_run_sqlite_work() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let package = package("1.0.0");
    repository.set_enabled(&package, true).unwrap();

    let executor = repository.executor.clone();
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let blocker = tokio::spawn(async move {
        executor
            .execute(move |_| {
                entered_tx.send(()).expect("executor entry observed");
                release_rx.recv().expect("executor released");
                Ok::<_, InfrastructureError>(())
            })
            .await
    });
    entered_rx.await.expect("SQLite executor occupied");

    let mut snapshot = Box::pin(repository.freeze_for_listener_start_async(&package));
    assert!(matches!(poll_once(snapshot.as_mut()).await, Poll::Pending));
    let (progress_tx, progress_rx) = oneshot::channel();
    tokio::spawn(async move {
        progress_tx.send(()).expect("progress receiver alive");
    });
    progress_rx
        .await
        .expect("current-thread runtime progresses");
    drop(snapshot);

    release_tx.send(()).expect("release SQLite executor");
    blocker.await.unwrap().unwrap();
    repository
        .freeze_for_listener_start_async(&package)
        .await
        .expect("later snapshot succeeds after queued cancellation");
}
