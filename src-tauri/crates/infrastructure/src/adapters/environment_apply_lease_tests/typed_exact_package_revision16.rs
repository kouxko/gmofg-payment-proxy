use super::*;

const DESCRIPTION: [u8; 32] = [0x38; 32];

fn typed_package(
    service_epoch: u64,
    description_fingerprint: [u8; 32],
    online_generation: u64,
    lease_generation: u64,
) -> EnvironmentExactPackageBaseline {
    EnvironmentExactPackageBaseline::observed_projection(
        ProtocolPackageRef {
            id: ProtocolPackageId::new("revision16-package").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        },
        Uuid::from_u128(0x38),
        true,
        true,
        service_epoch,
        description_fingerprint,
        online_generation,
        lease_generation,
    )
}

fn typed_observation(
    service_epoch: u64,
    description_fingerprint: [u8; 32],
    online_generation: u64,
    lease_generation: u64,
) -> EnvironmentApplyLeaseResourceObservation {
    EnvironmentApplyLeaseResourceObservation::ExactPackage {
        generation: Uuid::from_u128(0x38),
        enabled: true,
        online: true,
        service_epoch,
        description_fingerprint,
        online_generation,
        lease_generation,
    }
}

async fn assert_typed_projection_drift_is_package_stale(
    observation: EnvironmentApplyLeaseResourceObservation,
) {
    let runtime = Arc::new(FakeRuntime::default());
    let request = request(
        generations(),
        Vec::new(),
        Vec::new(),
        vec![typed_package(10, DESCRIPTION, 20, 30)],
    );
    configure(&runtime, &request);
    runtime.set(package_key("revision16-package", "1.0.0"), observation);

    let lease = EnvironmentApplyLeaseAdapter::new(runtime)
        .acquire(request)
        .await
        .expect("typed package drift returns a lease outcome");

    assert!(lease.is_package_stale());
    assert!(!lease.is_generation_mismatch());
}

#[test]
fn exact_package_baseline_exposes_every_revision16_projection_field() {
    let package = typed_package(10, DESCRIPTION, 20, 30);

    assert_eq!(package.service_epoch(), 10);
    assert_eq!(package.description_fingerprint(), &DESCRIPTION);
    assert_eq!(package.online_generation(), 20);
    assert_eq!(package.lease_generation(), 30);
}

#[tokio::test]
async fn service_epoch_drift_has_package_stale_precedence() {
    assert_typed_projection_drift_is_package_stale(typed_observation(11, DESCRIPTION, 20, 30))
        .await;
}

#[tokio::test]
async fn description_fingerprint_drift_has_package_stale_precedence() {
    assert_typed_projection_drift_is_package_stale(typed_observation(10, [0x39; 32], 20, 30)).await;
}

#[tokio::test]
async fn online_generation_drift_has_package_stale_precedence() {
    assert_typed_projection_drift_is_package_stale(typed_observation(10, DESCRIPTION, 21, 30))
        .await;
}

#[tokio::test]
async fn lease_generation_drift_has_package_stale_precedence() {
    assert_typed_projection_drift_is_package_stale(typed_observation(10, DESCRIPTION, 20, 31))
        .await;
}
