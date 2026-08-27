use std::{future::Future, pin::Pin, sync::Arc, task::Poll};

use intercept_proxy_application::{
    EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest, ExternalPackageApplicationPort,
    ListenerRuntimePort, ProtocolPackageRef,
};
use intercept_proxy_domain::ExternalPackageRegistration;
use intercept_proxy_domain::{ProxyListener, ProxyWorkspace};

use super::{
    AndroidAdbAdapter, EnvironmentApplyLeaseAdapter, EnvironmentApplyLeaseRuntime,
    EnvironmentApplyResourceGateRegistry, ExternalPackageClient, ExternalPackageConnectionId,
    ExternalPackageRegistryAdapter, ListenerRuntimeAdapter,
};

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

fn wire_real_adapters_to_one_registry(
    runtime: Arc<dyn EnvironmentApplyLeaseRuntime>,
    listener: ListenerRuntimeAdapter,
    android: AndroidAdbAdapter,
    packages: ExternalPackageRegistryAdapter,
) {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let _lease = EnvironmentApplyLeaseAdapter::with_resource_gates(runtime, gates.clone());
    let _listener = listener.with_environment_apply_resource_gates(gates.clone());
    let _android = android.with_environment_apply_resource_gates(gates.clone());
    let _packages = packages.with_environment_apply_resource_gates(gates);
}

async fn listener_start_is_blocked_by_real_apply_lease(
    lease_adapter: &EnvironmentApplyLeaseAdapter,
    listener_runtime: &ListenerRuntimeAdapter,
    request: EnvironmentApplyLeaseRequest,
    workspace: ProxyWorkspace,
    listener: ProxyListener,
) {
    let lease = lease_adapter.acquire(request).await.unwrap();
    let mut mutation = Box::pin(listener_runtime.start(workspace, listener));
    assert!(poll_once(mutation.as_mut()).is_pending());
    drop(lease);
    let _ = mutation.await;
}

async fn listener_stop_cleanup_is_blocked_by_real_apply_lease(
    lease_adapter: &EnvironmentApplyLeaseAdapter,
    listener_runtime: &ListenerRuntimeAdapter,
    request: EnvironmentApplyLeaseRequest,
    listener: ProxyListener,
) {
    let lease = lease_adapter.acquire(request).await.unwrap();
    let mut mutation = Box::pin(listener_runtime.stop(listener.id));
    assert!(poll_once(mutation.as_mut()).is_pending());
    drop(lease);
    let _ = mutation.await;
}

async fn external_disconnect_publication_is_blocked_by_real_apply_lease(
    lease_adapter: &EnvironmentApplyLeaseAdapter,
    packages: &ExternalPackageRegistryAdapter,
    request: EnvironmentApplyLeaseRequest,
    package: ProtocolPackageRef,
) {
    let lease = lease_adapter.acquire(request).await.unwrap();
    let mut mutation = Box::pin(packages.disconnect(&package));
    assert!(poll_once(mutation.as_mut()).is_pending());
    drop(lease);
    let _ = mutation.await;
}

async fn external_online_generation_publication_is_blocked_by_real_apply_lease(
    lease_adapter: &EnvironmentApplyLeaseAdapter,
    packages: &ExternalPackageRegistryAdapter,
    request: EnvironmentApplyLeaseRequest,
    registration: ExternalPackageRegistration,
    fingerprint: [u8; 32],
    client: ExternalPackageClient,
) {
    let lease = lease_adapter.acquire(request).await.unwrap();
    let mut mutation = Box::pin(packages.accept_registration(&registration, fingerprint, client));
    assert!(poll_once(mutation.as_mut()).is_pending());
    drop(lease);
    let _ = mutation.await;
}

async fn external_offline_generation_publication_is_blocked_by_real_apply_lease(
    lease_adapter: &EnvironmentApplyLeaseAdapter,
    packages: &ExternalPackageRegistryAdapter,
    request: EnvironmentApplyLeaseRequest,
    package: ProtocolPackageRef,
    connection_id: ExternalPackageConnectionId,
) {
    let lease = lease_adapter.acquire(request).await.unwrap();
    let mut mutation = Box::pin(packages.mark_disconnected(&package, connection_id));
    assert!(poll_once(mutation.as_mut()).is_pending());
    drop(lease);
    let _ = mutation.await;
}

#[test]
fn real_adapter_gate_behavior_contracts_are_linked() {
    let _ = wire_real_adapters_to_one_registry;
    let _ = listener_start_is_blocked_by_real_apply_lease;
    let _ = listener_stop_cleanup_is_blocked_by_real_apply_lease;
    let _ = external_disconnect_publication_is_blocked_by_real_apply_lease;
    let _ = external_online_generation_publication_is_blocked_by_real_apply_lease;
    let _ = external_offline_generation_publication_is_blocked_by_real_apply_lease;
}

#[test]
fn bundle_constructs_one_registry_and_wires_every_real_mutator() {
    let source = include_str!("bundle.rs");
    assert!(source.contains("EnvironmentApplyResourceGateRegistry::default"));
    for required in [
        "listener_runtime.with_environment_apply_resource_gates",
        "android.with_environment_apply_resource_gates",
        "external_packages.with_environment_apply_resource_gates",
        "EnvironmentApplyLeaseAdapter::with_resource_gates",
    ] {
        assert!(source.contains(required), "bundle misses `{required}`");
    }
}
