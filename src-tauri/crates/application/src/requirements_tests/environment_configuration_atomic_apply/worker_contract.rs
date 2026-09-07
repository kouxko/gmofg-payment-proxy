use crate::{
    EnvironmentApplyGenerations, EnvironmentApplyLease, EnvironmentApplyLeaseOutcome,
    EnvironmentApplyLeasePort, EnvironmentApplyWork, EnvironmentCommitPort,
    EnvironmentCommitReceipt, EnvironmentProtectedMaterialPreparePort,
};

fn assert_port_is_application_boundary<T: ?Sized + Send + Sync + 'static>() {}

#[test]
fn apply_dependencies_are_send_sync_application_ports() {
    assert_port_is_application_boundary::<dyn EnvironmentApplyLeasePort>();
    assert_port_is_application_boundary::<dyn EnvironmentProtectedMaterialPreparePort>();
    assert_port_is_application_boundary::<dyn EnvironmentCommitPort>();
}

#[test]
fn successful_apply_completion_requires_an_unforgeable_commit_receipt() {
    let finish: fn(EnvironmentApplyWork, EnvironmentCommitReceipt) -> _ =
        EnvironmentApplyWork::finish_committed;
    let _ = finish;
}

#[test]
fn lease_exposes_a_typed_acquired_phase_outcome() {
    let lease = EnvironmentApplyLease::acquired(EnvironmentApplyGenerations::default());

    assert_eq!(lease.outcome(), EnvironmentApplyLeaseOutcome::Acquired);
}

#[test]
fn lease_exposes_a_typed_package_stale_phase_outcome() {
    let lease = EnvironmentApplyLease::package_stale(EnvironmentApplyGenerations::default());

    assert_eq!(lease.outcome(), EnvironmentApplyLeaseOutcome::PackageStale);
}

#[test]
fn lease_exposes_a_typed_generation_mismatch_phase_outcome() {
    let lease = EnvironmentApplyLease::generation_mismatch(EnvironmentApplyGenerations::default());

    assert_eq!(
        lease.outcome(),
        EnvironmentApplyLeaseOutcome::GenerationMismatch
    );
}
