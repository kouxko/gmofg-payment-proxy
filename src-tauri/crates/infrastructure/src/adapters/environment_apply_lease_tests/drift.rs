use super::*;

#[tokio::test]
async fn active_listener_returns_typed_runtime_active_failure() {
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
            runtime_epoch: Some(Uuid::from_u128(0x100)),
            active_count: 1,
        },
    );

    let lease = EnvironmentApplyLeaseAdapter::new(runtime)
        .acquire(request)
        .await
        .expect("active Listener uses a typed lease outcome");
    assert_eq!(
        lease.outcome(),
        intercept_proxy_application::EnvironmentApplyLeaseOutcome::RuntimeActive
    );
}

#[tokio::test]
async fn any_android_owner_state_returns_typed_owner_active_failure() {
    let runtime = Arc::new(FakeRuntime::default());
    let request = request(
        generations(),
        Vec::new(),
        vec![EnvironmentAndroidOwnerBaseline::observed(
            "profile-g038".into(),
            "DEVICE-G038".into(),
            Uuid::from_u128(0xa0),
            "preparing".into(),
        )],
        Vec::new(),
    );
    configure(&runtime, &request);

    let lease = EnvironmentApplyLeaseAdapter::new(runtime)
        .acquire(request)
        .await
        .expect("Android owner mismatch uses a typed lease outcome");
    assert_eq!(
        lease.outcome(),
        intercept_proxy_application::EnvironmentApplyLeaseOutcome::AndroidOwnerMismatch
    );
}

#[tokio::test]
async fn package_disappearance_is_typed_stale_not_generic_unavailable() {
    let runtime = Arc::new(FakeRuntime::default());
    let request = request(
        generations(),
        Vec::new(),
        Vec::new(),
        vec![package("au-eftex", 0x10)],
    );
    configure(&runtime, &request);
    runtime.make_unavailable(package_key("au-eftex", "1.0.0"));

    let lease = EnvironmentApplyLeaseAdapter::new(runtime)
        .acquire(request)
        .await
        .expect("package disappearance has a typed stale outcome");
    assert!(lease.is_package_stale());
}

#[tokio::test]
async fn exact_package_scope_uses_semver_canonical_order() {
    let runtime = Arc::new(FakeRuntime::default());
    let request = request(
        generations(),
        Vec::new(),
        Vec::new(),
        vec![
            package_version("pkg", "1.10.0", 0x10),
            package_version("pkg", "1.2.0", 0x20),
        ],
    );
    configure(&runtime, &request);

    let lease = EnvironmentApplyLeaseAdapter::new(runtime.clone())
        .acquire(request)
        .await
        .unwrap();

    assert_eq!(
        *runtime.acquired.lock().unwrap(),
        vec![package_key("pkg", "1.2.0"), package_key("pkg", "1.10.0"),]
    );
    drop(lease);
}

#[tokio::test]
async fn cancellation_while_waiting_for_later_gate_releases_owned_prefix_in_reverse() {
    let runtime = Arc::new(FakeRuntime::default());
    let adapter = EnvironmentApplyLeaseAdapter::new(runtime.clone());
    let later = request(
        generations(),
        vec![listener(0x30, 0x300, 0)],
        Vec::new(),
        Vec::new(),
    );
    configure(&runtime, &later);
    let held_later = adapter.acquire(later).await.unwrap();
    runtime.acquired.lock().unwrap().clear();
    runtime.released.lock().unwrap().clear();
    let multi = request(
        generations(),
        vec![listener(0x30, 0x300, 0), listener(0x10, 0x100, 0)],
        Vec::new(),
        Vec::new(),
    );
    configure(&runtime, &multi);
    let mut pending = Box::pin(adapter.acquire(multi));
    assert!(poll_once(pending.as_mut()).is_pending());

    drop(pending);

    assert_eq!(
        *runtime.released.lock().unwrap(),
        vec![EnvironmentApplyLeaseResourceKey::Listener(Uuid::from_u128(
            0x10
        ))]
    );
    drop(held_later);
}

#[tokio::test]
async fn restored_package_values_with_a_new_projection_generation_are_stale() {
    let expected = generations();
    let runtime = Arc::new(FakeRuntime::with_generations(EnvironmentApplyGenerations {
        package: expected.package + 1,
        ..expected.clone()
    }));
    let request = request(
        expected,
        Vec::new(),
        Vec::new(),
        vec![package("au-eftex", 0x10)],
    );
    configure(&runtime, &request);

    let lease = EnvironmentApplyLeaseAdapter::new(runtime)
        .acquire(request)
        .await
        .unwrap();

    assert!(lease.is_package_stale());
    assert!(!lease.is_generation_mismatch());
}

#[test]
fn android_owner_keys_use_profile_then_original_serial_order() {
    let mut keys = vec![
        EnvironmentApplyLeaseResourceKey::AndroidOwner {
            profile_id: "profile-g038".into(),
            serial: "DEVICE-10".into(),
        },
        EnvironmentApplyLeaseResourceKey::AndroidOwner {
            profile_id: "profile-g038".into(),
            serial: "DEVICE-02".into(),
        },
    ];

    keys.sort_by(super::super::environment_apply_resources::canonical_resource_cmp);

    assert_eq!(
        keys,
        vec![
            EnvironmentApplyLeaseResourceKey::AndroidOwner {
                profile_id: "profile-g038".into(),
                serial: "DEVICE-02".into(),
            },
            EnvironmentApplyLeaseResourceKey::AndroidOwner {
                profile_id: "profile-g038".into(),
                serial: "DEVICE-10".into(),
            },
        ]
    );
}

#[test]
fn exact_package_gate_key_holds_a_validated_protocol_package_ref() {
    let package = intercept_proxy_application::ProtocolPackageRef {
        id: intercept_proxy_application::ProtocolPackageId::new("au-eftex").unwrap(),
        version: intercept_proxy_application::ProtocolPackageVersion::new("1.0.0").unwrap(),
    };

    let key = EnvironmentApplyLeaseResourceKey::ExactPackage(package.clone());

    assert_eq!(key.package_ref(), Some(&package));
}
