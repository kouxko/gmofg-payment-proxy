use super::*;

#[tokio::test]
async fn empty_baseline_still_scopes_a_current_owner_and_returns_owner_mismatch() {
    let runtime = Arc::new(FakeRuntime::default());
    let request = request(generations(), Vec::new(), Vec::new(), Vec::new());
    configure(&runtime, &request);
    let owner = android_owner("profile-current", "DEVICE-CURRENT", 0xc0);
    runtime.set_android_owners(vec![owner.clone()]);
    let key = EnvironmentApplyLeaseResourceKey::AndroidOwner {
        profile_id: owner.profile_id,
        serial: owner.serial.clone(),
    };
    runtime.set(
        key.clone(),
        EnvironmentApplyLeaseResourceObservation::Android {
            serial: owner.serial,
            owner_epoch: Some(owner.epoch),
            state: "active".into(),
        },
    );

    let lease = EnvironmentApplyLeaseAdapter::new(runtime.clone())
        .acquire(request)
        .await
        .unwrap();

    assert_eq!(*runtime.acquired.lock().unwrap(), vec![key]);
    assert_eq!(
        lease.outcome(),
        intercept_proxy_application::EnvironmentApplyLeaseOutcome::AndroidOwnerMismatch
    );
}

#[test]
fn android_resource_observation_must_match_the_exact_serial() {
    let request = request(
        generations(),
        Vec::new(),
        vec![EnvironmentAndroidOwnerBaseline::observed(
            "profile-a".into(),
            "DEVICE-A".into(),
            Uuid::from_u128(0xa0),
            "active".into(),
        )],
        Vec::new(),
    );
    let key = EnvironmentApplyLeaseResourceKey::AndroidOwner {
        profile_id: "profile-a".into(),
        serial: "DEVICE-A".into(),
    };
    let wrong_device = EnvironmentApplyLeaseResourceObservation::Android {
        serial: "DEVICE-B".into(),
        owner_epoch: Some(Uuid::from_u128(0xa0)),
        state: "active".into(),
    };

    assert!(
        !super::super::environment_configuration_lease::resource_matches(
            &request.validated_baseline,
            &key,
            &wrong_device,
        )
    );
}

#[tokio::test]
async fn android_owner_aba_is_a_generation_mismatch() {
    let runtime = Arc::new(FakeRuntime::default());
    let request = full_request();
    configure(&runtime, &request);
    runtime.set(
        EnvironmentApplyLeaseResourceKey::AndroidOwner {
            profile_id: "profile-g038".into(),
            serial: "DEVICE-G038".into(),
        },
        EnvironmentApplyLeaseResourceObservation::Android {
            serial: "DEVICE-G038".into(),
            owner_epoch: Some(Uuid::from_u128(0xa1)),
            state: "inactive".into(),
        },
    );

    let lease = EnvironmentApplyLeaseAdapter::new(runtime)
        .acquire(request)
        .await
        .unwrap();

    assert_eq!(
        lease.outcome(),
        intercept_proxy_application::EnvironmentApplyLeaseOutcome::AndroidOwnerMismatch
    );
}
