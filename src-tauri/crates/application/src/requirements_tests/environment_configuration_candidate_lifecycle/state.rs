use super::support::*;

#[test]
fn validating_status_has_no_public_snapshot_before_g035_supplies_one() {
    let registry = registry();
    let admitted = insert_validating(&registry, "Store Lab", 1);

    let status = json(&registry.status(admitted.candidate_id()));

    assert_eq!(status["status"], "validating");
    assert!(status["target_key"].is_null());
    assert!(status["baseline_public"].is_null());
    assert_eq!(status["validation_layers"], serde_json::json!([]));
    assert!(status["preview"].is_null());
}

#[test]
fn preview_ready_create_serializes_the_exact_typed_g033_snapshot() {
    let registry = registry();
    let admitted = insert_validating(&registry, "Store Lab", 1);

    let ready = registry
        .complete_preview_ready(admitted.candidate_id(), public_snapshot())
        .expect("typed snapshot completes validation");
    let actual = json(&ready);
    let expected = expected_preview_value();

    assert_eq!(actual["target_key"], expected["target_key"]);
    assert_eq!(actual["baseline_public"], expected["baseline_public"]);
    assert_eq!(actual["validation_layers"], expected["validation_layers"]);
    assert_eq!(actual["preview"]["target"], expected["target"]);
    assert_eq!(actual["preview"]["resources"], expected["resources"]);
    assert_eq!(actual["preview"]["alias_graph"], expected["alias_graph"]);
    assert_eq!(
        actual["preview"]["materials_public"],
        expected["materials_public"]
    );
}

#[test]
fn preview_ready_status_retains_the_exact_typed_g033_snapshot() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Store Lab", 1);

    let actual = json(&registry.status(ready.candidate_id()));
    let expected = expected_preview_value();

    assert_eq!(actual["target_key"], expected["target_key"]);
    assert_eq!(actual["baseline_public"], expected["baseline_public"]);
    assert_eq!(actual["validation_layers"], expected["validation_layers"]);
    assert_eq!(actual["preview"]["target"], expected["target"]);
    assert_eq!(actual["preview"]["resources"], expected["resources"]);
    assert_eq!(
        actual["preview"]["protocol_document_values"],
        expected["protocol_document_values"]
    );
}

#[test]
fn mismatched_snapshot_target_cannot_mint_a_confirmation_token() {
    let registry = registry();
    let admitted = insert_validating(&registry, "Admitted Target", 1);

    let result = registry.complete_preview_ready(
        admitted.candidate_id(),
        public_snapshot_named("Different Target"),
    );

    assert!(result.is_err());
    assert_eq!(
        registry.status(admitted.candidate_id()).status(),
        EnvironmentCandidateStatus::Validating
    );
}

#[test]
fn public_target_key_uses_g033_canonical_format_without_trusting_wire_key() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Case Sensitive Lab", 1);

    let status = json(&registry.status(ready.candidate_id()));

    assert_eq!(status["target_key"], "new:case sensitive lab");
}

#[test]
fn validation_failed_status_retains_typed_layers_and_diagnostics() {
    let registry = registry();
    let admitted = insert_validating(&registry, "Validation Failure", 1);

    fail_validation(&registry, admitted.candidate_id())
        .expect("typed validation failure becomes terminal");
    let status = json(&registry.status(admitted.candidate_id()));

    assert_eq!(status["status"], "validation_failed");
    assert_eq!(status["validation_layers"][0]["layer"], "domain");
    assert_eq!(status["validation_layers"][0]["status"], "failed");
    assert_eq!(
        status["validation_layers"][0]["reason"],
        "environment validation layer failed"
    );
    assert_eq!(
        status["errors"][0]["message"],
        "environment validation layer failed"
    );
}

#[test]
fn wrong_confirmation_token_does_not_consume_the_candidate_token() {
    let registry = registry();
    let first = admit_preview_ready(&registry, "First Token", 1);
    let second = admit_preview_ready(&registry, "Second Token", 1);
    let wrong = token_from_create(&second);

    let error = registry
        .queue_apply(first.candidate_id(), &wrong)
        .expect_err("another candidate token is rejected");

    assert_eq!(
        error,
        EnvironmentCandidateLifecycleError::ConfirmationTokenInvalid
    );
    registry
        .queue_apply(first.candidate_id(), &token_from_create(&first))
        .expect("the original token remains usable");
}

#[test]
fn consumed_token_reuse_returns_token_consumed_while_queued() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Queued Token", 1);
    let token = token_from_create(&ready);
    registry
        .queue_apply(ready.candidate_id(), &token)
        .expect("first use queues work");

    assert!(matches!(
        registry.queue_apply(ready.candidate_id(), &token),
        Err(EnvironmentCandidateLifecycleError::TokenConsumed)
    ));
}

#[test]
fn consumed_token_reuse_returns_token_consumed_while_in_progress() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "In Progress Token", 1);
    let token = token_from_create(&ready);
    registry
        .queue_apply(ready.candidate_id(), &token)
        .expect("first use queues work");
    let _work = registry
        .claim_next_apply()
        .expect("FIFO claim succeeds")
        .expect("queued work exists");

    assert!(matches!(
        registry.queue_apply(ready.candidate_id(), &token),
        Err(EnvironmentCandidateLifecycleError::TokenConsumed)
    ));
}

#[test]
fn consumed_token_reuse_returns_token_consumed_after_terminal_completion() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Terminal Token", 1);
    let token = token_from_create(&ready);
    registry
        .queue_apply(ready.candidate_id(), &token)
        .expect("first use queues work");
    registry
        .claim_next_apply()
        .expect("FIFO claim succeeds")
        .expect("queued work exists")
        .finish_failed_before_commit(EnvironmentStatusCode::CommitFailed)
        .expect("guard terminalizes");

    assert!(matches!(
        registry.queue_apply(ready.candidate_id(), &token),
        Err(EnvironmentCandidateLifecycleError::TokenConsumed)
    ));
}

#[test]
fn consumed_token_marker_disappears_only_after_terminal_eviction() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Evicted Token 00", 1);
    let token = token_from_create(&ready);
    registry
        .queue_apply(ready.candidate_id(), &token)
        .expect("first use queues work");
    registry
        .claim_next_apply()
        .expect("FIFO claim succeeds")
        .expect("queued work exists")
        .finish_failed_before_commit(EnvironmentStatusCode::CommitFailed)
        .expect("guard terminalizes");
    for index in 1..=32 {
        let admitted = insert_validating(&registry, &format!("Evicted Token {index:02}"), 1);
        registry.cancel(admitted.candidate_id());
    }

    assert!(matches!(
        registry.queue_apply(ready.candidate_id(), &token),
        Err(EnvironmentCandidateLifecycleError::CandidateNotFound)
    ));
}

#[test]
fn status_never_serializes_a_confirmation_token() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Status Token", 1);
    let token = json(&ready)["confirmation_token"]
        .as_str()
        .expect("create exposes token")
        .to_owned();

    let status = json(&registry.status(ready.candidate_id()));

    assert!(status.get("confirmation_token").is_none());
    assert!(!status.to_string().contains(&token));
}

#[test]
fn cancel_result_marks_successful_cancel_as_terminal() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Cancel Terminal", 1);

    let cancel = json(&registry.cancel(ready.candidate_id()));

    assert_eq!(cancel["status"], "cancelled");
    assert_eq!(cancel["terminal"], true);
}

#[test]
fn cancel_result_marks_in_progress_apply_as_non_terminal() {
    let registry = registry();
    let (ready, _work) = claim_apply(&registry, "Cancel In Progress");

    let cancel = json(&registry.cancel(ready.candidate_id()));

    assert_eq!(cancel["status"], "apply_in_progress_not_cancellable");
    assert_eq!(cancel["terminal"], false);
}

#[test]
fn cancel_result_marks_known_terminal_candidate_as_terminal() {
    let registry = registry();
    let ready = admit_preview_ready(&registry, "Known Terminal", 1);
    registry.cancel(ready.candidate_id());

    let cancel = json(&registry.cancel(ready.candidate_id()));

    assert_eq!(cancel["status"], "not_found_or_terminal");
    assert_eq!(cancel["terminal"], true);
}

#[test]
fn cancel_result_marks_absent_candidate_as_non_terminal() {
    let registry = registry();
    let missing = EnvironmentCandidateId::new("missing-candidate").expect("valid candidate ID");

    let cancel = json(&registry.cancel(&missing));

    assert_eq!(cancel["status"], "not_found_or_terminal");
    assert_eq!(cancel["terminal"], false);
}
