use std::{future::Future, pin::Pin, task::Poll};

use super::*;

async fn poll_once<F>(mut future: Pin<&mut F>) -> Poll<F::Output>
where
    F: Future,
{
    std::future::poll_fn(|context| Poll::Ready(future.as_mut().poll(context))).await
}

#[tokio::test]
async fn application_snapshot_uses_one_workspace_aggregate_read_without_n_plus_one() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(SnapshotProbeWorkspaceRepository::default());
    let application = application_with_workspace_repository(Arc::clone(&ports), workspaces.clone());

    let first = application.application_snapshot().await.expect("snapshot");
    let second = application.application_snapshot().await.expect("snapshot");

    assert_eq!(first.workspaces.len(), first.workspace_details.len());
    assert_eq!(first.generation, second.generation);
    assert_eq!(workspaces.snapshot_calls.load(Ordering::SeqCst), 2);
    assert_eq!(workspaces.list_calls.load(Ordering::SeqCst), 0);
    assert_eq!(workspaces.get_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ports.settings_get_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn application_snapshot_holds_the_mutation_gate_until_the_aggregate_read_finishes() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(SnapshotProbeWorkspaceRepository::default());
    workspaces.block_snapshot.store(true, Ordering::SeqCst);
    let application = Arc::new(application_with_workspace_repository(
        Arc::clone(&ports),
        workspaces.clone(),
    ));

    let snapshot = tokio::spawn({
        let application = Arc::clone(&application);
        async move { application.application_snapshot().await }
    });
    workspaces.snapshot_entered.notified().await;

    let mut save = Box::pin(application.settings_save(valid_settings_draft()));
    assert!(matches!(poll_once(save.as_mut()).await, Poll::Pending));
    assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 0);

    workspaces.continue_snapshot.notify_one();
    snapshot.await.expect("snapshot joined").expect("snapshot");
    save.await.expect("settings save");
    assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 1);
}
