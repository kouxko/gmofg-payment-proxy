use super::support::*;
use crate::EnvironmentCandidatePolicy;

const TERMINAL_BYTES_BUDGET: usize = 4_194_304;

#[test]
fn default_policy_matches_required_capacity_and_retention_budgets() {
    let policy = EnvironmentCandidatePolicy::default();

    assert_eq!(policy.max_active_candidates(), 4);
    assert_eq!(policy.max_active_per_target(), 1);
    assert_eq!(policy.max_active_apply_global(), 1);
    assert_eq!(policy.max_active_apply_per_target(), 1);
    assert_eq!(policy.max_terminal_candidates(), 32);
    assert_eq!(policy.max_terminal_public_bytes(), TERMINAL_BYTES_BUDGET);
    assert_eq!(
        EnvironmentCandidatePolicy::eviction_policy(),
        "oldest_first"
    );
}

#[test]
fn terminal_count_retains_32_and_evicts_oldest_at_33() {
    let registry = registry();
    let mut ids = Vec::new();
    for index in 0..32 {
        let admitted = insert_validating(&registry, &format!("Count Target {index:02}"), 1);
        ids.push(admitted.candidate_id().clone());
        registry.cancel(admitted.candidate_id());
    }
    assert_eq!(registry.metrics().retained_terminal_candidates(), 32);

    let thirty_third = insert_validating(&registry, "Count Target 32", 1);
    registry.cancel(thirty_third.candidate_id());

    assert_eq!(
        registry.status(&ids[0]).status(),
        EnvironmentCandidateStatus::NotFound
    );
    assert_eq!(registry.metrics().retained_terminal_candidates(), 32);
}

#[test]
fn cumulative_terminal_bytes_retain_exact_4_mib_and_evict_at_b_plus_one() {
    let probe = registry();
    let probe_candidate = insert_validating(&probe, "Byte Target", 1);
    complete_preview_ready(
        &probe,
        probe_candidate.candidate_id(),
        public_snapshot_with_padding("Byte Target", 0),
    )
    .expect("typed public probe snapshot validates");
    probe.cancel(probe_candidate.candidate_id());
    let base_bytes = probe.metrics().terminal_public_bytes();
    let half_budget = TERMINAL_BYTES_BUDGET / 2;
    assert!(base_bytes < half_budget);
    let public_padding_bytes = half_budget - base_bytes;

    let exact = registry();
    let exact_first = insert_validating(&exact, "Byte Target", 1);
    complete_preview_ready(
        &exact,
        exact_first.candidate_id(),
        public_snapshot_with_padding("Byte Target", public_padding_bytes),
    )
    .expect("first typed terminal snapshot validates");
    exact.cancel(exact_first.candidate_id());
    assert_eq!(exact.metrics().terminal_public_bytes(), half_budget);

    let exact_second = insert_validating(&exact, "Byte Target", 1);
    complete_preview_ready(
        &exact,
        exact_second.candidate_id(),
        public_snapshot_with_padding("Byte Target", public_padding_bytes),
    )
    .expect("second typed terminal snapshot validates");
    exact.cancel(exact_second.candidate_id());
    assert_eq!(
        exact.metrics().terminal_public_bytes(),
        TERMINAL_BYTES_BUDGET
    );
    assert_eq!(
        exact.status(exact_first.candidate_id()).status(),
        EnvironmentCandidateStatus::Cancelled
    );

    let overflow = registry();
    let overflow_first = insert_validating(&overflow, "Byte Target", 1);
    complete_preview_ready(
        &overflow,
        overflow_first.candidate_id(),
        public_snapshot_with_padding("Byte Target", public_padding_bytes),
    )
    .expect("first half-budget terminal snapshot validates");
    overflow.cancel(overflow_first.candidate_id());
    let overflow_second = insert_validating(&overflow, "Byte Target", 1);
    complete_preview_ready(
        &overflow,
        overflow_second.candidate_id(),
        public_snapshot_with_padding("Byte Target", public_padding_bytes + 1),
    )
    .expect("exact B+1 cumulative snapshot validates");
    overflow.cancel(overflow_second.candidate_id());

    assert_eq!(
        overflow.status(overflow_first.candidate_id()).status(),
        EnvironmentCandidateStatus::NotFound
    );
    assert_eq!(
        overflow.status(overflow_second.candidate_id()).status(),
        EnvironmentCandidateStatus::Cancelled
    );
    assert_eq!(overflow.metrics().retained_terminal_candidates(), 1);
    assert_eq!(overflow.metrics().terminal_public_bytes(), half_budget + 1);
}

#[test]
fn terminal_overflow_never_evicts_any_active_lifecycle_state() {
    let registry = EnvironmentCandidateRegistry::new(
        EnvironmentCandidatePolicy::new(8, 1, 2, 1, 1, TERMINAL_BYTES_BUDGET)
            .expect("bounded test policy is valid"),
    );
    let validating = insert_validating(&registry, "Active Validating", 1);
    let preview = admit_preview_ready(&registry, "Active Preview", 1);
    let in_progress = admit_preview_ready(&registry, "Active In Progress", 1);
    registry
        .queue_apply(in_progress.candidate_id(), &token_from_create(&in_progress))
        .expect("work to be claimed queues first");
    let work = registry
        .claim_next_apply()
        .expect("FIFO claim succeeds")
        .expect("first queued work becomes in progress");
    let queued = admit_preview_ready(&registry, "Active Queued", 1);
    registry
        .queue_apply(queued.candidate_id(), &token_from_create(&queued))
        .expect("later work remains queued");
    let terminal_one = insert_validating(&registry, "Overflow Terminal 1", 1);
    registry.cancel(terminal_one.candidate_id());
    let terminal_two = insert_validating(&registry, "Overflow Terminal 2", 1);
    registry.cancel(terminal_two.candidate_id());

    assert_eq!(
        registry.status(validating.candidate_id()).status(),
        EnvironmentCandidateStatus::Validating
    );
    assert_eq!(
        registry.status(preview.candidate_id()).status(),
        EnvironmentCandidateStatus::PreviewReady
    );
    assert_eq!(
        registry.status(queued.candidate_id()).status(),
        EnvironmentCandidateStatus::ApplyQueued
    );
    assert_eq!(
        registry.status(in_progress.candidate_id()).status(),
        EnvironmentCandidateStatus::ApplyInProgress
    );
    drop(work);
}
