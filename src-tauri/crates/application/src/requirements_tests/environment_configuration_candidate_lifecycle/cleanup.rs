use super::support::*;

#[test]
fn validating_cancel_releases_private_memory_and_target_capacity() {
    let registry = registry();
    let admitted = insert_validating(&registry, "Validating Cleanup", 1);

    registry.cancel(admitted.candidate_id());

    assert_eq!(registry.metrics().private_candidate_bytes(), 0);
    insert_validating(&registry, "Validating Cleanup", 2);
}

#[test]
fn validation_failed_releases_private_memory_and_target_capacity() {
    let registry = registry();
    let admitted = insert_validating(&registry, "Validation Cleanup", 1);

    fail_validation(&registry, admitted.candidate_id())
        .expect("validation failure becomes terminal");

    assert_eq!(registry.metrics().private_candidate_bytes(), 0);
    insert_validating(&registry, "Validation Cleanup", 2);
}

#[test]
fn stale_invalidation_releases_private_memory_and_target_capacity() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Stale Cleanup", 7);

    assert!(
        registry
            .invalidate_if_epoch_changed(ready.candidate_id(), EnvironmentCandidateEpoch::new(8))
    );

    assert_eq!(registry.metrics().private_candidate_bytes(), 0);
    insert_validating(&registry, "Stale Cleanup", 8);
}

#[test]
fn preview_cancel_releases_private_memory_and_target_capacity() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Preview Cleanup", 1);

    registry.cancel(ready.candidate_id());

    assert_eq!(registry.metrics().private_candidate_bytes(), 0);
    insert_validating(&registry, "Preview Cleanup", 2);
}

#[test]
fn fresh_registry_does_not_restore_prior_process_candidates() {
    let first = registry();
    let admitted = insert_validating(&first, "Process Local", 1);
    let id = admitted.candidate_id().clone();
    drop(first);

    assert_eq!(
        registry().status(&id).status(),
        EnvironmentCandidateStatus::NotFound
    );
}

#[test]
fn shutdown_rejects_new_candidates() {
    let registry = registry();
    let _drain = registry.begin_shutdown();

    let error = registry
        .insert_validating(
            candidate_named("Shutdown Create"),
            EnvironmentCandidateEpoch::new(1),
        )
        .expect_err("shutdown rejects new candidates");

    assert_eq!(
        error,
        EnvironmentCandidateLifecycleError::ShutdownInProgress
    );
}

#[test]
fn shutdown_rejects_new_apply_requests() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Shutdown Apply", 1);
    let token = token_from_create(&ready);
    let _drain = registry.begin_shutdown();

    let error = registry
        .queue_apply(ready.candidate_id(), &token)
        .expect_err("shutdown rejects new apply requests");

    assert_eq!(
        error,
        EnvironmentCandidateLifecycleError::ShutdownInProgress
    );
}
