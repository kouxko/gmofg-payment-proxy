use std::{future::Future, pin::Pin, sync::Arc, task::Poll};

use intercept_proxy_application::ExternalPackageApplicationPort;

use super::*;
use crate::adapters::{EnvironmentApplyLeaseResourceKey, EnvironmentApplyResourceGateRegistry};

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

#[tokio::test(flavor = "current_thread")]
async fn record_connection_error_waits_for_the_exact_package_apply_gate() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()))
        .with_environment_apply_resource_gates(gates.clone());
    let registration = registration("Revision 16 error gate");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 61).await;
    let accepted = registry
        .accept_registration(&registration, fingerprint, client)
        .await
        .unwrap();
    let guard = gates
        .acquire(EnvironmentApplyLeaseResourceKey::ExactPackage(
            package.clone(),
        ))
        .await;

    let reason = PackageTransportError::Disconnected;
    let mut publication =
        Box::pin(registry.record_connection_error(&package, accepted.connection_id, &reason));
    assert!(poll_once(publication.as_mut()).is_pending());

    drop(guard);
    assert!(publication.await.unwrap());
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_disconnect_keeps_the_exact_package_gate_until_cleanup_finishes() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let registry = Arc::new(
        ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()))
            .with_environment_apply_resource_gates(gates.clone()),
    );
    let registration = registration("Revision 16 cancellation gate");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 62).await;
    registry
        .accept_registration(&registration, fingerprint, client)
        .await
        .unwrap();
    let (reached, release) = registry.install_disconnect_barrier(package.clone());
    let caller = tokio::spawn({
        let registry = registry.clone();
        let package = package.clone();
        async move { registry.disconnect(&package).await }
    });
    reached.notified().await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());

    let mut competing = Box::pin(
        gates.acquire(EnvironmentApplyLeaseResourceKey::ExactPackage(
            package.clone(),
        )),
    );
    assert!(poll_once(competing.as_mut()).is_pending());

    release.notify_one();
    registry.cleanup_complete.notified().await;
    drop(competing.await);
}

#[tokio::test(flavor = "current_thread")]
async fn service_listening_publication_waits_for_every_known_exact_package_gate() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()))
        .with_environment_apply_resource_gates(gates.clone());
    let registration = registration("Revision 16 service listening gate");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 63).await;
    registry
        .accept_registration(&registration, fingerprint, client)
        .await
        .unwrap();
    let guard = gates
        .acquire(EnvironmentApplyLeaseResourceKey::ExactPackage(
            package.clone(),
        ))
        .await;

    let mut publication = Box::pin(registry.mark_service_listening("ws://127.0.0.1:9000/packages"));
    assert!(poll_once(publication.as_mut()).is_pending());

    drop(guard);
    publication.await;
    assert!(matches!(
        registry.service_status().await.unwrap().state,
        intercept_proxy_application::ExternalPackageServiceStateViewModel::Listening
    ));
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn service_failed_publication_waits_for_every_known_exact_package_gate() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()))
        .with_environment_apply_resource_gates(gates.clone());
    let registration = registration("Revision 16 service failed gate");
    let package = registration.package().identity().clone();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 64).await;
    registry
        .accept_registration(&registration, fingerprint, client)
        .await
        .unwrap();
    let guard = gates
        .acquire(EnvironmentApplyLeaseResourceKey::ExactPackage(
            package.clone(),
        ))
        .await;

    let mut publication =
        Box::pin(registry.mark_service_failed("ws://127.0.0.1:9000/packages", "bind failed"));
    assert!(poll_once(publication.as_mut()).is_pending());

    drop(guard);
    publication.await;
    assert!(matches!(
        registry.service_status().await.unwrap().state,
        intercept_proxy_application::ExternalPackageServiceStateViewModel::Failed { .. }
    ));
    registry.disconnect(&package).await.unwrap();
}
