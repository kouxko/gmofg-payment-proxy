use super::support::*;
use crate::EnvironmentCandidatePolicy;

#[test]
fn active_candidate_budget_rejects_the_fifth_distinct_target() {
    let registry = registry();
    for index in 0..4 {
        insert_validating(&registry, &format!("Global Target {index}"), 1);
    }

    let error = registry
        .insert_validating(
            candidate_named("Global Target 4"),
            EnvironmentCandidateEpoch::new(1),
        )
        .expect_err("the fifth active candidate exceeds the global budget");

    assert_eq!(
        error,
        EnvironmentCandidateLifecycleError::CandidateCapacityExceeded
    );
}

#[test]
fn same_candidate_cannot_forge_a_second_target_identity() {
    let registry = registry();
    registry
        .insert_validating(candidate(), EnvironmentCandidateEpoch::new(1))
        .expect("first candidate is admitted");

    let error = registry
        .insert_validating(candidate(), EnvironmentCandidateEpoch::new(2))
        .expect_err("registry derives the same internal identity from the same candidate");

    assert_eq!(
        error,
        EnvironmentCandidateLifecycleError::TargetCandidateAlreadyActive
    );
}

#[test]
fn new_target_identity_trims_display_name_before_capacity_check() {
    let registry = registry();
    insert_validating(&registry, "  Store Lab  ", 1);

    let error = registry
        .insert_validating(
            candidate_named("Store Lab"),
            EnvironmentCandidateEpoch::new(2),
        )
        .expect_err("trim-equivalent names share one internal target identity");

    assert_eq!(
        error,
        EnvironmentCandidateLifecycleError::TargetCandidateAlreadyActive
    );
}

#[test]
fn new_target_identity_preserves_case_exact_utf8_bytes() {
    let registry = registry();
    insert_validating(&registry, "Store Lab", 1);

    registry
        .insert_validating(
            candidate_named("STORE LAB"),
            EnvironmentCandidateEpoch::new(1),
        )
        .expect("case-distinct UTF-8 bytes define another target identity");
}

#[test]
fn new_target_identity_preserves_nfc_and_nfd_utf8_bytes() {
    let registry = registry();
    insert_validating(&registry, "Café", 1);

    registry
        .insert_validating(
            candidate_named("Cafe\u{301}"),
            EnvironmentCandidateEpoch::new(1),
        )
        .expect("normalization-distinct UTF-8 bytes define another target identity");
}

#[test]
fn global_apply_capacity_releases_only_after_guard_terminal_completion() {
    let registry = registry();
    let (_first, work) = claim_apply(&registry, "Apply One");
    let second = admit_preview_ready(&registry, "Apply Two", 1);

    let blocked = registry
        .queue_apply(second.candidate_id(), &token_from_create(&second))
        .expect_err("in-progress guard retains global apply capacity");
    assert_eq!(
        blocked,
        EnvironmentCandidateLifecycleError::ApplyAlreadyActive
    );

    work.finish_failed_before_commit(EnvironmentStatusCode::CommitFailed)
        .expect("guard reaches terminal completion");
    registry
        .queue_apply(second.candidate_id(), &token_from_create(&second))
        .expect("terminal completion releases global apply capacity");
}

#[test]
fn target_apply_capacity_releases_only_after_guard_terminal_completion() {
    let registry = EnvironmentCandidateRegistry::new(
        EnvironmentCandidatePolicy::new(4, 2, 2, 1, 32, 4_194_304)
            .expect("bounded test policy is valid"),
    );
    let first = admit_preview_ready(&registry, "Apply Target", 1);
    let second = admit_preview_ready(&registry, "Apply Target", 2);
    registry
        .queue_apply(first.candidate_id(), &token_from_create(&first))
        .expect("first apply queues");
    let work = registry
        .claim_next_apply()
        .expect("FIFO claim succeeds")
        .expect("queued work exists");

    let blocked = registry
        .queue_apply(second.candidate_id(), &token_from_create(&second))
        .expect_err("in-progress guard retains per-target apply capacity");
    assert_eq!(
        blocked,
        EnvironmentCandidateLifecycleError::ApplyAlreadyActive
    );

    work.finish_failed_before_commit(EnvironmentStatusCode::CommitFailed)
        .expect("guard reaches terminal completion");
    registry
        .queue_apply(second.candidate_id(), &token_from_create(&second))
        .expect("terminal completion releases per-target apply capacity");
}
