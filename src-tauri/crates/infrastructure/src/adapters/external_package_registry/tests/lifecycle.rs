use std::{future::Future, pin::Pin, task::Poll};

use super::*;

fn registration_with_id(name: &str, id: &str) -> ExternalPackageRegistration {
    let mut value = serde_json::to_value(registration(name)).unwrap();
    value["package"]["id"] = serde_json::Value::String(id.to_owned());
    serde_json::from_value(value).unwrap()
}

async fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    let mut future = future;
    std::future::poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))).await
}

async fn register(
    registry: &ExternalPackageRegistryAdapter,
    registration: &ExternalPackageRegistration,
    generation: u64,
) -> (ExternalPackageConnectionId, Peer) {
    let (client, peer) = connected_client(registration, generation).await;
    let accepted = registry
        .accept_registration(
            registration,
            external_package_registration_fingerprint(registration).unwrap(),
            client,
        )
        .await
        .unwrap();
    (accepted.connection_id, peer)
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_disconnect_cleanup_survives_and_other_package_progresses() {
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    let first = registration_with_id("slow close", "slow-close");
    let first_package = first.package().identity().clone();
    let (_first_id, _first_peer) = register(&registry, &first, 1).await;
    let (reached, release) = registry.install_disconnect_barrier(first_package.clone());
    let reached_wait = reached.notified();
    let mut disconnect = Box::pin(registry.disconnect(&first_package));
    assert!(matches!(
        poll_once(disconnect.as_mut()).await,
        Poll::Pending
    ));
    reached_wait.await;
    let mut first_completion = match registry.online.lock().get(&first_package) {
        Some(OnlineConnection::Closing { completion, .. }) => completion.clone(),
        _ => panic!("first package must be Closing"),
    };

    let second = registration_with_id("independent", "independent-close");
    let second_package = second.package().identity().clone();
    let (_second_id, _second_peer) = register(&registry, &second, 2).await;
    registry.disconnect(&second_package).await.unwrap();

    drop(disconnect);
    release.notify_one();
    ExternalPackageRegistryAdapter::wait_for_closing(&mut first_completion).await;
    let (_replacement_id, _replacement_peer) = register(&registry, &first, 3).await;
    registry.disconnect(&first_package).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_delete_finishes_owned_cleanup_and_allows_fresh_install() {
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    let registration = registration_with_id("delete", "cancelled-delete");
    let package = registration.package().identity().clone();
    let (_connection_id, _peer) = register(&registry, &registration, 10).await;
    let (reached, release) = registry.install_disconnect_barrier(package.clone());
    let reached_wait = reached.notified();
    let deletion_wait = registry.deletion_complete.notified();
    let mut deletion = Box::pin(registry.delete(&package));
    assert!(matches!(poll_once(deletion.as_mut()).await, Poll::Pending));
    reached_wait.await;
    drop(deletion);
    release.notify_one();
    deletion_wait.await;
    assert!(registry.get(&package).await.unwrap().is_none());

    let (_replacement_id, _replacement_peer) = register(&registry, &registration, 11).await;
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn stale_cleanup_cannot_remove_a_new_connection_generation() {
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    let registration = registration_with_id("generation", "stale-cleanup");
    let package = registration.package().identity().clone();
    let (_old_id, _old_peer) = register(&registry, &registration, 20).await;
    let (reached, release) = registry.install_disconnect_barrier(package.clone());
    let reached_wait = reached.notified();
    let mut disconnect = Box::pin(registry.disconnect(&package));
    assert!(matches!(
        poll_once(disconnect.as_mut()).await,
        Poll::Pending
    ));
    reached_wait.await;
    let mut completion = match registry.online.lock().get(&package) {
        Some(OnlineConnection::Closing { completion, .. }) => completion.clone(),
        _ => panic!("old generation must be Closing"),
    };

    let (new_client, _new_peer) = connected_client(&registration, 21).await;
    let new_id = ExternalPackageConnectionId::new();
    registry.online.lock().insert(
        package.clone(),
        OnlineConnection::Active {
            id: new_id,
            client: new_client,
        },
    );
    drop(disconnect);
    release.notify_one();
    ExternalPackageRegistryAdapter::wait_for_closing(&mut completion).await;
    assert!(matches!(
        registry.online.lock().get(&package),
        Some(OnlineConnection::Active { id, .. }) if *id == new_id
    ));
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn delete_database_failure_releases_the_exact_closing_generation() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let registry = ExternalPackageRegistryAdapter::new(Arc::clone(&store));
    let registration = registration_with_id("delete failure", "delete-failure");
    let package = registration.package().identity().clone();
    let (_connection_id, _peer) = register(&registry, &registration, 30).await;
    store.remove_external_package_table_for_test();

    let error = registry.delete(&package).await.unwrap_err();

    assert_eq!(error.view_model.code, "INTERNAL_ERROR");
    assert!(
        registry.online.lock().get(&package).is_none(),
        "failed deletion retained a Closing generation"
    );
}
