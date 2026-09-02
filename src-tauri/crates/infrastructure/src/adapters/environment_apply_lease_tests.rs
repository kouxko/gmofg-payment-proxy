use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use async_trait::async_trait;
use futures_util::task::noop_waker_ref;
use intercept_proxy_application::{
    AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState,
    AndroidRuntimeOwnerTransitionReason, AndroidRuntimeOwnerViewModel, AppResult,
    EnvironmentAffectedListenerBaseline, EnvironmentAndroidOwnerBaseline,
    EnvironmentApplyGenerations, EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest,
    EnvironmentCandidateEpoch, EnvironmentCandidateId, EnvironmentExactPackageBaseline,
    EnvironmentMaterialInventoryBaseline, EnvironmentValidatedApplyBaselineCollector,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use tokio::sync::Notify;
use uuid::Uuid;

use super::{
    EnvironmentApplyLeaseAdapter, EnvironmentApplyLeaseResourceKey,
    EnvironmentApplyLeaseResourceObservation, EnvironmentApplyLeaseRuntime,
};

mod android_owners;
mod drift;
#[path = "environment_apply_lease_tests/typed_exact_package_revision16.rs"]
mod typed_exact_package_revision16;

type PauseState = Option<(EnvironmentApplyLeaseResourceKey, Arc<Notify>, Arc<Notify>)>;

struct FakeRuntime {
    generations: Mutex<EnvironmentApplyGenerations>,
    observations:
        Mutex<BTreeMap<EnvironmentApplyLeaseResourceKey, EnvironmentApplyLeaseResourceObservation>>,
    acquired: Mutex<Vec<EnvironmentApplyLeaseResourceKey>>,
    released: Mutex<Vec<EnvironmentApplyLeaseResourceKey>>,
    unavailable: Mutex<BTreeSet<EnvironmentApplyLeaseResourceKey>>,
    pause: Mutex<PauseState>,
    android_owners: Mutex<Vec<AndroidRuntimeOwnerViewModel>>,
}

impl Default for FakeRuntime {
    fn default() -> Self {
        Self {
            generations: Mutex::new(generations()),
            observations: Mutex::new(BTreeMap::new()),
            acquired: Mutex::new(Vec::new()),
            released: Mutex::new(Vec::new()),
            unavailable: Mutex::new(BTreeSet::new()),
            pause: Mutex::new(None),
            android_owners: Mutex::new(Vec::new()),
        }
    }
}

impl FakeRuntime {
    fn with_generations(generations: EnvironmentApplyGenerations) -> Self {
        Self {
            generations: Mutex::new(generations),
            ..Self::default()
        }
    }

    fn set(
        &self,
        key: EnvironmentApplyLeaseResourceKey,
        value: EnvironmentApplyLeaseResourceObservation,
    ) {
        self.observations.lock().unwrap().insert(key, value);
    }

    fn pause_on(&self, key: EnvironmentApplyLeaseResourceKey) -> (Arc<Notify>, Arc<Notify>) {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        *self.pause.lock().unwrap() = Some((key, entered.clone(), release.clone()));
        (entered, release)
    }

    fn make_unavailable(&self, key: EnvironmentApplyLeaseResourceKey) {
        self.unavailable.lock().unwrap().insert(key);
    }

    fn set_android_owners(&self, owners: Vec<AndroidRuntimeOwnerViewModel>) {
        *self.android_owners.lock().unwrap() = owners;
    }
}

#[async_trait]
impl EnvironmentApplyLeaseRuntime for FakeRuntime {
    async fn observe_generations(
        &self,
        _workspace_id: Uuid,
    ) -> AppResult<EnvironmentApplyGenerations> {
        Ok(self.generations.lock().unwrap().clone())
    }

    async fn observe_resource(
        &self,
        key: &EnvironmentApplyLeaseResourceKey,
    ) -> AppResult<EnvironmentApplyLeaseResourceObservation> {
        if self.unavailable.lock().unwrap().contains(key) {
            return Err(intercept_proxy_application::AppError::new(
                "FAKE_RESOURCE_UNAVAILABLE",
                "resource unavailable",
            ));
        }
        let pause = self.pause.lock().unwrap().take();
        if let Some((pause_key, entered, release)) = pause {
            if pause_key == *key {
                entered.notify_one();
                release.notified().await;
            } else {
                *self.pause.lock().unwrap() = Some((pause_key, entered, release));
            }
        }
        Ok(self.observations.lock().unwrap()[key].clone())
    }

    async fn observe_android_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        Ok(self.android_owners.lock().unwrap().clone())
    }

    fn resource_acquired(&self, key: &EnvironmentApplyLeaseResourceKey) {
        self.acquired.lock().unwrap().push(key.clone());
    }

    fn resource_released(&self, key: &EnvironmentApplyLeaseResourceKey) {
        self.released.lock().unwrap().push(key.clone());
    }
}

fn listener(id: u128, epoch: u128, active: u32) -> EnvironmentAffectedListenerBaseline {
    EnvironmentAffectedListenerBaseline::observed(
        Uuid::from_u128(id),
        Some(Uuid::from_u128(epoch)),
        active,
    )
}

fn package(id: &str, generation: u128) -> EnvironmentExactPackageBaseline {
    package_version(id, "1.0.0", generation)
}

fn package_version(id: &str, version: &str, generation: u128) -> EnvironmentExactPackageBaseline {
    EnvironmentExactPackageBaseline::observed(
        ProtocolPackageRef {
            id: ProtocolPackageId::new(id.to_owned()).unwrap(),
            version: ProtocolPackageVersion::new(version.to_owned()).unwrap(),
        },
        Uuid::from_u128(generation),
        true,
        true,
    )
}

fn package_key(id: &str, version: &str) -> EnvironmentApplyLeaseResourceKey {
    EnvironmentApplyLeaseResourceKey::ExactPackage(ProtocolPackageRef {
        id: ProtocolPackageId::new(id.to_owned()).unwrap(),
        version: ProtocolPackageVersion::new(version.to_owned()).unwrap(),
    })
}

fn android_owner(profile_id: &str, serial: &str, epoch: u128) -> AndroidRuntimeOwnerViewModel {
    AndroidRuntimeOwnerViewModel {
        serial: serial.to_owned(),
        epoch: Uuid::from_u128(epoch),
        mode: AndroidRuntimeOwnerMode::AdbReverse,
        profile_id: profile_id.to_owned(),
        state: AndroidRuntimeOwnerState::Active,
        source: AndroidRuntimeOwnerSource::Start,
        transition_reason: AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        updated_at: chrono::Utc::now(),
    }
}

fn generations() -> EnvironmentApplyGenerations {
    EnvironmentApplyGenerations {
        selected_workspace_id: Some(Uuid::from_u128(0x3800)),
        listener: 1,
        android: 2,
        package: 3,
        package_inventory: 4,
        certificate_inventory: 5,
        protected_secret_inventory: 6,
        application_mutation: 7,
    }
}

fn request(
    generations: EnvironmentApplyGenerations,
    listeners: Vec<EnvironmentAffectedListenerBaseline>,
    android: Vec<EnvironmentAndroidOwnerBaseline>,
    packages: Vec<EnvironmentExactPackageBaseline>,
) -> EnvironmentApplyLeaseRequest {
    EnvironmentApplyLeaseRequest {
        candidate_id: EnvironmentCandidateId::new("g038-candidate").unwrap(),
        candidate_epoch: EnvironmentCandidateEpoch::new(38),
        expected: generations.clone(),
        validated_baseline: EnvironmentValidatedApplyBaselineCollector::collect(
            Uuid::from_u128(0x3800),
            generations,
            [38; 32],
            listeners,
            android,
            packages,
            vec![EnvironmentMaterialInventoryBaseline::observed(
                "certificate:g038".into(),
                [0x38; 32],
            )],
        )
        .expect("full baseline collects"),
    }
}

fn configure(runtime: &FakeRuntime, request: &EnvironmentApplyLeaseRequest) {
    for listener in request.validated_baseline.affected_listeners() {
        runtime.set(
            EnvironmentApplyLeaseResourceKey::Listener(listener.listener_id()),
            EnvironmentApplyLeaseResourceObservation::Listener {
                runtime_epoch: listener.runtime_epoch(),
                active_count: listener.active_count(),
            },
        );
    }
    let mut owners = Vec::new();
    for android in request.validated_baseline.android_owners() {
        runtime.set(
            EnvironmentApplyLeaseResourceKey::AndroidOwner {
                profile_id: android.profile_id().to_owned(),
                serial: android.serial().to_owned(),
            },
            EnvironmentApplyLeaseResourceObservation::Android {
                serial: android.serial().to_owned(),
                owner_epoch: Some(android.owner_epoch()),
                state: android.state().to_owned(),
            },
        );
        owners.push(AndroidRuntimeOwnerViewModel {
            serial: android.serial().to_owned(),
            epoch: android.owner_epoch(),
            mode: AndroidRuntimeOwnerMode::AdbReverse,
            profile_id: android.profile_id().to_owned(),
            state: AndroidRuntimeOwnerState::Active,
            source: AndroidRuntimeOwnerSource::Start,
            transition_reason: AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
            updated_at: chrono::Utc::now(),
        });
    }
    *runtime.android_owners.lock().unwrap() = owners;
    for package in request.validated_baseline.exact_packages() {
        runtime.set(
            package_key(package.package_id(), package.version()),
            EnvironmentApplyLeaseResourceObservation::ExactPackage {
                generation: package.generation(),
                enabled: package.enabled(),
                online: package.online(),
                service_epoch: package.service_epoch(),
                description_fingerprint: *package.description_fingerprint(),
                online_generation: package.online_generation(),
                lease_generation: package.lease_generation(),
            },
        );
    }
}

fn full_request() -> EnvironmentApplyLeaseRequest {
    request(
        generations(),
        vec![listener(0x30, 0x300, 0), listener(0x10, 0x100, 0)],
        vec![EnvironmentAndroidOwnerBaseline::observed(
            "profile-g038".into(),
            "DEVICE-G038".into(),
            Uuid::from_u128(0xa0),
            "active".into(),
        )],
        vec![package("z-package", 0x20), package("a-package", 0x10)],
    )
}

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    let mut context = Context::from_waker(noop_waker_ref());
    future.poll(&mut context)
}

#[tokio::test]
async fn adapter_acquires_affected_resources_in_canonical_order() {
    let runtime = Arc::new(FakeRuntime::default());
    let request = full_request();
    configure(&runtime, &request);
    let adapter = EnvironmentApplyLeaseAdapter::new(runtime.clone());

    let lease = adapter.acquire(request).await.unwrap();

    assert_eq!(
        *runtime.acquired.lock().unwrap(),
        vec![
            EnvironmentApplyLeaseResourceKey::Listener(Uuid::from_u128(0x10)),
            EnvironmentApplyLeaseResourceKey::Listener(Uuid::from_u128(0x30)),
            EnvironmentApplyLeaseResourceKey::AndroidOwner {
                profile_id: "profile-g038".into(),
                serial: "DEVICE-G038".into(),
            },
            package_key("a-package", "1.0.0"),
            package_key("z-package", "1.0.0"),
        ]
    );
    drop(lease);
}

#[tokio::test]
async fn acquired_lease_holds_the_owned_resource_gate_until_drop() {
    let runtime = Arc::new(FakeRuntime::default());
    let first_request = full_request();
    configure(&runtime, &first_request);
    let adapter = EnvironmentApplyLeaseAdapter::new(runtime);
    let first = adapter.acquire(first_request).await.unwrap();
    let mut second = Box::pin(adapter.acquire(full_request()));

    assert!(poll_once(second.as_mut()).is_pending());
    drop(first);
    assert!(poll_once(second.as_mut()).is_ready());
}

#[tokio::test]
async fn release_callback_observes_reverse_canonical_order() {
    let runtime = Arc::new(FakeRuntime::default());
    let request = full_request();
    configure(&runtime, &request);
    let lease = EnvironmentApplyLeaseAdapter::new(runtime.clone())
        .acquire(request)
        .await
        .unwrap();
    let acquired = runtime.acquired.lock().unwrap().clone();

    drop(lease);

    let mut expected = acquired;
    expected.reverse();
    assert_eq!(*runtime.released.lock().unwrap(), expected);
}

#[tokio::test]
async fn package_stale_wins_when_package_and_global_generation_both_changed() {
    let expected = generations();
    let runtime = Arc::new(FakeRuntime::with_generations(EnvironmentApplyGenerations {
        application_mutation: expected.application_mutation + 1,
        ..expected.clone()
    }));
    let request = request(
        expected,
        Vec::new(),
        Vec::new(),
        vec![package("au-eftex", 0x10)],
    );
    configure(&runtime, &request);
    runtime.set(
        package_key("au-eftex", "1.0.0"),
        EnvironmentApplyLeaseResourceObservation::ExactPackage {
            generation: Uuid::from_u128(0x11),
            enabled: true,
            online: true,
            service_epoch: 0,
            description_fingerprint: [0; 32],
            online_generation: 0,
            lease_generation: 0,
        },
    );

    let lease = EnvironmentApplyLeaseAdapter::new(runtime)
        .acquire(request)
        .await
        .unwrap();

    assert!(lease.is_package_stale());
    assert!(!lease.is_generation_mismatch());
}

#[tokio::test]
async fn listener_runtime_aba_is_a_generation_mismatch() {
    let runtime = Arc::new(FakeRuntime::default());
    let request = request(
        generations(),
        vec![listener(0x10, 0x100, 0)],
        Vec::new(),
        Vec::new(),
    );
    configure(&runtime, &request);
    runtime.set(
        EnvironmentApplyLeaseResourceKey::Listener(Uuid::from_u128(0x10)),
        EnvironmentApplyLeaseResourceObservation::Listener {
            runtime_epoch: Some(Uuid::from_u128(0x101)),
            active_count: 0,
        },
    );

    let lease = EnvironmentApplyLeaseAdapter::new(runtime)
        .acquire(request)
        .await
        .unwrap();

    assert!(lease.is_generation_mismatch());
    assert!(!lease.is_package_stale());
}

#[tokio::test]
async fn global_generation_aba_is_a_generation_mismatch() {
    let expected = generations();
    let runtime = Arc::new(FakeRuntime::with_generations(EnvironmentApplyGenerations {
        listener: expected.listener + 1,
        ..expected.clone()
    }));
    let request = request(expected, Vec::new(), Vec::new(), Vec::new());
    configure(&runtime, &request);

    let lease = EnvironmentApplyLeaseAdapter::new(runtime)
        .acquire(request)
        .await
        .unwrap();

    assert!(lease.is_generation_mismatch());
}

#[tokio::test]
async fn cancelling_during_acquisition_releases_already_owned_gates() {
    let runtime = Arc::new(FakeRuntime::default());
    let blocked_request = full_request();
    configure(&runtime, &blocked_request);
    let blocked_key = EnvironmentApplyLeaseResourceKey::Listener(Uuid::from_u128(0x30));
    let (entered, release) = runtime.pause_on(blocked_key);
    let adapter = EnvironmentApplyLeaseAdapter::new(runtime.clone());
    let acquire = tokio::spawn({
        let adapter = adapter.clone();
        async move { adapter.acquire(blocked_request).await }
    });
    entered.notified().await;

    acquire.abort();
    assert!(acquire.await.unwrap_err().is_cancelled());
    release.notify_one();
    let single = request(
        generations(),
        vec![listener(0x10, 0x100, 0)],
        Vec::new(),
        Vec::new(),
    );
    configure(&runtime, &single);
    assert!(adapter.acquire(single).await.is_ok());
}

#[tokio::test]
async fn disjoint_scopes_can_acquire_while_another_scope_is_paused() {
    let runtime = Arc::new(FakeRuntime::default());
    let first = request(
        generations(),
        vec![listener(0x10, 0x100, 0)],
        Vec::new(),
        Vec::new(),
    );
    configure(&runtime, &first);
    let (entered, release) = runtime.pause_on(EnvironmentApplyLeaseResourceKey::Listener(
        Uuid::from_u128(0x10),
    ));
    let adapter = EnvironmentApplyLeaseAdapter::new(runtime.clone());
    let paused = tokio::spawn({
        let adapter = adapter.clone();
        async move { adapter.acquire(first).await }
    });
    entered.notified().await;
    let second = request(
        generations(),
        vec![listener(0x20, 0x200, 0)],
        Vec::new(),
        Vec::new(),
    );
    configure(&runtime, &second);

    assert!(adapter.acquire(second).await.is_ok());
    release.notify_one();
    assert!(paused.await.unwrap().is_ok());
}

#[test]
fn service_bundle_wires_the_real_lease_adapter_into_application_dependencies() {
    let source = include_str!("bundle.rs");
    assert_eq!(
        source
            .matches("EnvironmentApplyLeaseAdapter::with_resource_gates")
            .count(),
        1
    );
    assert!(!source.contains("EnvironmentApplyLeaseAdapter::new"));
    assert!(source.contains("environment_apply_lease:"));
}
