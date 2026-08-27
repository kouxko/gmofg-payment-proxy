use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, mpsc},
    task::Poll,
};

use tokio::{sync::oneshot, task::JoinHandle};

use super::*;
use crate::InfrastructureError;

fn registration_with_id(name: &str, id: &str) -> ExternalPackageRegistration {
    let mut value = serde_json::to_value(registration(name)).expect("serialize registration");
    value["package"]["id"] = serde_json::Value::String(id.to_owned());
    serde_json::from_value(value).expect("registration with alternate identity")
}

async fn hold_sqlite_executor(
    registry: &ExternalPackageRegistryAdapter,
) -> (
    mpsc::Sender<()>,
    JoinHandle<Result<(), InfrastructureError>>,
) {
    let executor = registry.executor.clone();
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let blocker = tokio::spawn(async move {
        executor
            .execute(move |_| {
                entered_tx.send(()).expect("test still waits for entry");
                release_rx.recv().expect("test releases SQLite work");
                Ok::<_, InfrastructureError>(())
            })
            .await
    });
    entered_rx.await.expect("SQLite executor is occupied");
    (release_tx, blocker)
}

async fn release_sqlite_executor(
    release: mpsc::Sender<()>,
    blocker: JoinHandle<Result<(), InfrastructureError>>,
) {
    release.send(()).expect("release SQLite executor");
    blocker
        .await
        .expect("blocking task joined")
        .expect("blocking task succeeded");
}

async fn assert_current_thread_progresses() {
    let (progress_tx, progress_rx) = oneshot::channel();
    tokio::spawn(async move {
        progress_tx.send(()).expect("test still waits for progress");
    });
    progress_rx.await.expect("current-thread Tokio progressed");
}

async fn poll_once<F>(future: Pin<&mut F>) -> Poll<F::Output>
where
    F: Future,
{
    let mut future = future;
    std::future::poll_fn(|context| Poll::Ready(future.as_mut().poll(context))).await
}

#[tokio::test(flavor = "current_thread")]
async fn server_registry_persistence_uses_async_executor_without_locking_runtime_progress() {
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(store));
    let registration = registration_with_id("Slow registration", "slow-registration");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 1).await;

    let (release, blocker) = hold_sqlite_executor(&registry).await;
    let mut registration_future =
        Box::pin(registry.accept_registration(&registration, fingerprint, client));
    assert!(matches!(
        poll_once(registration_future.as_mut()).await,
        Poll::Pending
    ));
    assert_current_thread_progresses().await;

    let other_registration = registration_with_id("Other package", "other-package");
    let (other_client, _other_peer) = tokio::time::timeout(
        Duration::from_secs(1),
        connected_client(&other_registration, 2),
    )
    .await
    .expect("another package connection progresses while persistence waits");
    other_client.disconnect().await;

    release_sqlite_executor(release, blocker).await;
    let accepted = registration_future.await.expect("registration accepted");

    let (release, blocker) = hold_sqlite_executor(&registry).await;
    let mut address_future = Box::pin(registry.record_remote_address(
        &package,
        accepted.connection_id,
        "127.0.0.1:49152".parse().unwrap(),
    ));
    assert!(matches!(
        poll_once(address_future.as_mut()).await,
        Poll::Pending
    ));
    assert_current_thread_progresses().await;
    release_sqlite_executor(release, blocker).await;
    assert!(address_future.await.expect("address persisted"));

    let (release, blocker) = hold_sqlite_executor(&registry).await;
    let mut error_future = Box::pin(registry.record_connection_error(
        &package,
        accepted.connection_id,
        &ExternalPackageConnectionError::Disconnected,
    ));
    assert!(matches!(
        poll_once(error_future.as_mut()).await,
        Poll::Pending
    ));
    assert_current_thread_progresses().await;
    release_sqlite_executor(release, blocker).await;
    assert!(error_future.await.expect("error persisted"));

    registry.disconnect(&package).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_queued_registration_never_publishes_a_ghost_online_connection() {
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    let registry = ExternalPackageRegistryAdapter::new(Arc::clone(&store));
    let registration = registration_with_id("Cancelled registration", "cancelled-registration");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 3).await;

    let (release, blocker) = hold_sqlite_executor(&registry).await;
    let mut registration_future =
        Box::pin(registry.accept_registration(&registration, fingerprint, client));
    assert!(matches!(
        poll_once(registration_future.as_mut()).await,
        Poll::Pending
    ));
    drop(registration_future);
    assert!(registry.client(&package).is_none());

    release_sqlite_executor(release, blocker).await;
    assert!(
        store
            .get_external_package(&package)
            .expect("registration lookup")
            .is_none(),
        "cancelled queued persistence unexpectedly reached SQLite"
    );

    let (replacement, _replacement_peer) = connected_client(&registration, 4).await;
    registry
        .accept_registration(&registration, fingerprint, replacement)
        .await
        .expect("replacement registration is not blocked by cancelled work");
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_binding_resolution_waits_asynchronously_and_cancel_does_not_strand_executor() {
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    let registry = ExternalPackageRegistryAdapter::new(store);
    let registration = registration_with_id("Runtime binding", "runtime-binding");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 5).await;
    registry
        .accept_registration(&registration, fingerprint, client)
        .await
        .expect("registration accepted");
    registry
        .set_enabled(&package, true)
        .await
        .expect("package enabled");

    let (release, blocker) = hold_sqlite_executor(&registry).await;
    let mut resolution = Box::pin(ExternalSocketPackageProvider::resolve(&registry, &package));
    assert!(matches!(
        poll_once(resolution.as_mut()).await,
        Poll::Pending
    ));
    assert_current_thread_progresses().await;
    drop(resolution);

    release_sqlite_executor(release, blocker).await;
    let binding = ExternalSocketPackageProvider::resolve(&registry, &package)
        .await
        .expect("later resolution succeeds")
        .expect("enabled online package resolves");
    assert_eq!(binding.registration().package().identity(), &package);
    registry.disconnect(&package).await.unwrap();
}
