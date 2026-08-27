use std::{future::Future, pin::Pin, sync::Arc, task::Poll};

use async_trait::async_trait;
use intercept_proxy_application::{
    AndroidControlPort, AndroidDeviceTarget, AndroidRuntimeOwnerViewModel, AndroidRuntimeTarget,
    AppResult, EnvironmentAndroidOwnerBaseline, EnvironmentApplyGenerations,
    EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest, EnvironmentCandidateEpoch,
    EnvironmentCandidateId, EnvironmentMaterialInventoryBaseline,
    EnvironmentValidatedApplyBaselineCollector,
};
use intercept_proxy_domain::ListenerId;
use uuid::Uuid;

use super::super::*;
use super::{RecordingRunner, seed_active_runtime, test_activation};
use crate::adapters::{
    EnvironmentApplyLeaseAdapter, EnvironmentApplyLeaseResourceKey,
    EnvironmentApplyLeaseResourceObservation, EnvironmentApplyLeaseRuntime,
    EnvironmentApplyResourceGateRegistry,
};

#[derive(Debug)]
struct AndroidGateRuntime {
    generations: EnvironmentApplyGenerations,
}

#[async_trait]
impl EnvironmentApplyLeaseRuntime for AndroidGateRuntime {
    async fn observe_generations(&self, _: Uuid) -> AppResult<EnvironmentApplyGenerations> {
        Ok(self.generations.clone())
    }

    async fn observe_resource(
        &self,
        key: &EnvironmentApplyLeaseResourceKey,
    ) -> AppResult<EnvironmentApplyLeaseResourceObservation> {
        assert!(matches!(
            key,
            EnvironmentApplyLeaseResourceKey::AndroidOwner { .. }
        ));
        Ok(EnvironmentApplyLeaseResourceObservation::Android {
            serial: match key {
                EnvironmentApplyLeaseResourceKey::AndroidOwner { serial, .. } => serial.clone(),
                _ => unreachable!(),
            },
            owner_epoch: None,
            state: "inactive".into(),
        })
    }

    async fn observe_android_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        Ok(Vec::new())
    }
}

fn generations() -> EnvironmentApplyGenerations {
    EnvironmentApplyGenerations {
        selected_workspace_id: Some(Uuid::from_u128(0x38)),
        listener: 1,
        android: 2,
        package: 3,
        package_inventory: 4,
        certificate_inventory: 5,
        protected_secret_inventory: 6,
        application_mutation: 7,
    }
}

fn request(profile_id: Option<&str>, serial: Option<&str>) -> EnvironmentApplyLeaseRequest {
    let generations = generations();
    EnvironmentApplyLeaseRequest {
        candidate_id: EnvironmentCandidateId::new("android-gate-candidate").unwrap(),
        candidate_epoch: EnvironmentCandidateEpoch::new(38),
        expected: generations.clone(),
        validated_baseline: EnvironmentValidatedApplyBaselineCollector::collect(
            Uuid::from_u128(0x38),
            generations,
            [0x38; 32],
            Vec::new(),
            profile_id
                .zip(serial)
                .map(|(profile_id, serial)| {
                    vec![EnvironmentAndroidOwnerBaseline::observed(
                        profile_id.to_owned(),
                        serial.to_owned(),
                        Uuid::from_u128(0x3800),
                        "active".into(),
                    )]
                })
                .unwrap_or_default(),
            Vec::new(),
            vec![EnvironmentMaterialInventoryBaseline::observed(
                "certificate:android-gate".into(),
                [0x39; 32],
            )],
        )
        .unwrap(),
    }
}

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

async fn assert_network_apply_waits_for_lease(
    request: EnvironmentApplyLeaseRequest,
    profile_id: &str,
    device_serial: &str,
) {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let lease_adapter = EnvironmentApplyLeaseAdapter::with_resource_gates(
        Arc::new(AndroidGateRuntime {
            generations: generations(),
        }),
        gates.clone(),
    );
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let android = AndroidAdbAdapter::with_runner(temp.path(), runner.clone())
        .with_environment_apply_resource_gates(gates);
    let mut owner = super::runtime_owner(device_serial, AndroidRuntimeOwnerState::Active);
    owner.epoch = Uuid::from_u128(0x3800);
    owner.profile_id = profile_id.into();
    android.save_owner(owner).await.unwrap();
    runner.calls.lock().unwrap().clear();
    let lease = lease_adapter.acquire(request).await.unwrap();
    let activation = test_activation(profile_id, "203.0.113.10", ListenerId::new(), 8_443);

    let mut mutation = Box::pin(android.network_apply(
        AndroidRuntimeTarget {
            serial: device_serial.into(),
            expected_epoch: Uuid::from_u128(0x3800),
        },
        activation,
    ));
    assert!(poll_once(mutation.as_mut()).is_pending());
    assert!(runner.calls.lock().unwrap().is_empty());
    drop(lease);
    let _ = mutation.await;
    assert!(!runner.calls.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn network_apply_waits_for_the_profile_apply_lease_gate() {
    assert_network_apply_waits_for_lease(
        request(Some("profile-g038"), Some("DEVICE-G038")),
        "profile-g038",
        "DEVICE-G038",
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn network_apply_waits_for_the_device_apply_lease_gate() {
    assert_network_apply_waits_for_lease(
        request(Some("profile-g038"), Some("DEVICE-G038")),
        "profile-g038",
        "DEVICE-G038",
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn emergency_restore_waits_for_the_owner_profile_and_device_lease_gates() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let lease_adapter = EnvironmentApplyLeaseAdapter::with_resource_gates(
        Arc::new(AndroidGateRuntime {
            generations: generations(),
        }),
        gates.clone(),
    );
    let lease = lease_adapter
        .acquire(request(Some("profile-old"), Some("DEVICE-G038")))
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let android = AndroidAdbAdapter::with_runner(temp.path(), runner.clone())
        .with_environment_apply_resource_gates(gates);
    let (_, runtime) = seed_active_runtime(&android, "DEVICE-G038", vec![31_627]).await;

    let mut mutation = Box::pin(android.emergency_restore(AndroidRuntimeTarget {
        serial: "DEVICE-G038".into(),
        expected_epoch: runtime.epoch,
    }));
    assert!(poll_once(mutation.as_mut()).is_pending());
    assert!(runner.calls.lock().unwrap().is_empty());
    drop(lease);
    let _ = mutation.await;
    assert!(!runner.calls.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn network_status_publication_waits_for_the_owner_profile_and_device_lease_gates() {
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let lease_adapter = EnvironmentApplyLeaseAdapter::with_resource_gates(
        Arc::new(AndroidGateRuntime {
            generations: generations(),
        }),
        gates.clone(),
    );
    let lease = lease_adapter
        .acquire(request(Some("profile-old"), Some("DEVICE-G038")))
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let android = AndroidAdbAdapter::with_runner(temp.path(), runner.clone())
        .with_environment_apply_resource_gates(gates);
    seed_active_runtime(&android, "DEVICE-G038", vec![31_627]).await;

    let mut mutation = Box::pin(android.network_status(AndroidDeviceTarget {
        serial: "DEVICE-G038".into(),
    }));
    assert!(poll_once(mutation.as_mut()).is_pending());
    assert!(runner.calls.lock().unwrap().is_empty());
    drop(lease);
    let _ = mutation.await;
    assert!(!runner.calls.lock().unwrap().is_empty());
}
