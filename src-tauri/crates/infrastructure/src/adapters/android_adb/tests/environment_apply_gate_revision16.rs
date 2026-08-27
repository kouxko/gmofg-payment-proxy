use std::{future::Future, pin::Pin, sync::Arc, task::Poll};

use intercept_proxy_application::{
    AndroidControlPort, AndroidDeviceTarget, AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource,
    AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason, AndroidRuntimeOwnerViewModel,
};

use super::super::*;
use super::{RecordingRunner, seed_active_runtime};
use crate::adapters::EnvironmentApplyResourceGateRegistry;

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

fn owner(epoch: uuid::Uuid) -> AndroidRuntimeOwnerViewModel {
    AndroidRuntimeOwnerViewModel {
        serial: "DEVICE-G038".into(),
        epoch,
        mode: AndroidRuntimeOwnerMode::AdbReverse,
        profile_id: "profile-g038".into(),
        state: AndroidRuntimeOwnerState::Active,
        source: AndroidRuntimeOwnerSource::Start,
        transition_reason: AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        updated_at: chrono::Utc::now(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn endpoint_reconciliation_waits_for_the_owner_profile_and_device_apply_gate() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let key = EnvironmentApplyResourceGateRegistry::android_owner_key("profile-old", "DEVICE-G038");
    let guard = gates.acquire(key).await;
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let android = AndroidAdbAdapter::with_runner(temp.path(), runner)
        .with_environment_apply_resource_gates(gates);
    seed_active_runtime(&android, "DEVICE-G038", vec![31_627]).await;

    let mut reconciliation = Box::pin(android.network_runtime_endpoints(
        AndroidDeviceTarget {
            serial: "DEVICE-G038".into(),
        },
        None,
    ));
    assert!(poll_once(reconciliation.as_mut()).is_pending());

    drop(guard);
    reconciliation.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn idle_owner_scope_still_uses_the_requested_profile_and_selected_device() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let key =
        EnvironmentApplyResourceGateRegistry::android_owner_key("profile-g038", "DEVICE-G038");
    let guard = gates.acquire(key).await;
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let android = AndroidAdbAdapter::with_runner(temp.path(), runner)
        .with_environment_apply_resource_gates(gates);
    *android.selected_serial.write().unwrap() = Some("DEVICE-G038".into());

    let mut status = Box::pin(android.network_status(AndroidDeviceTarget {
        serial: "DEVICE-G038".into(),
    }));
    assert!(poll_once(status.as_mut()).is_pending());

    drop(guard);
    let _ = status.await;
}

#[tokio::test(flavor = "current_thread")]
async fn owner_none_present_none_hidden_aba_advances_without_intermediate_observation() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let before = gates.reconcile_android_projections(Vec::new());
    let temp = tempfile::tempdir().unwrap();
    let android = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(RecordingRunner::default()))
        .with_environment_apply_resource_gates(gates.clone());
    let epoch = uuid::Uuid::new_v4();

    android.save_owner(owner(epoch)).await.unwrap();
    assert!(
        android
            .clear_owner_if_epoch("DEVICE-G038", epoch)
            .await
            .unwrap()
    );
    let after_aba = gates.reconcile_android_projections(Vec::new());

    assert!(
        before < after_aba,
        "save-clear must publish mutations and retain a tombstone without intermediate observation"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn stale_owner_clear_waits_for_gate_and_does_not_advance_generation() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let before = gates.reconcile_android_projections(Vec::new());
    let key =
        EnvironmentApplyResourceGateRegistry::android_owner_key("profile-g038", "DEVICE-G038");
    let guard = gates.acquire(key).await;
    let temp = tempfile::tempdir().unwrap();
    let android = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(RecordingRunner::default()))
        .with_environment_apply_resource_gates(gates.clone());

    let mut stale_clear =
        Box::pin(android.clear_owner_if_epoch("DEVICE-G038", uuid::Uuid::new_v4()));
    assert!(poll_once(stale_clear.as_mut()).is_pending());
    drop(guard);
    assert!(!stale_clear.await.unwrap());
    let after = gates.reconcile_android_projections(Vec::new());

    assert_eq!(
        before, after,
        "failed/no-op mutation must not advance generation"
    );
}
