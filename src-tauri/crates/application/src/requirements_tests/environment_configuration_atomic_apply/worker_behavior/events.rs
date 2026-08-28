use super::*;

fn environment_commit_events(events: &crate::EventHub) -> Vec<crate::UiEventEnvelope> {
    events
        .replay_after(0)
        .events
        .into_iter()
        .filter(|event| {
            matches!(
                event.payload,
                crate::UiEventPayload::SnapshotRequired { ref reason }
                    if reason == "environment_configuration_committed"
            )
        })
        .collect()
}

fn commit_fixture(
    outcome: CommitOutcome,
) -> (
    EnvironmentCandidateRegistry,
    crate::EnvironmentCandidateId,
    Arc<FakeLease>,
    Arc<FakePrepare>,
    Arc<FakeCommit>,
) {
    let (registry, candidate_id) = queued_registry();
    (
        registry,
        candidate_id,
        Arc::new(FakeLease::new(LeaseOutcome::Acquired(
            EnvironmentApplyGenerations::default(),
        ))),
        Arc::new(FakePrepare {
            outcome: PrepareOutcome::Success,
            calls: AtomicUsize::new(0),
        }),
        Arc::new(FakeCommit {
            outcome,
            calls: AtomicUsize::new(0),
            baselines: Mutex::new(Vec::new()),
        }),
    )
}

#[tokio::test]
async fn successful_commit_publishes_one_authoritative_workspace_refresh_event() {
    let (registry, candidate_id, lease, prepare, commit) = commit_fixture(CommitOutcome::Success);

    let events = run_worker(registry.clone(), lease, prepare, commit).await;

    assert_eq!(status(&registry, &candidate_id)["status"], "committed");
    let commit_events = environment_commit_events(&events);
    assert_eq!(commit_events.len(), 1);
    assert_eq!(
        commit_events[0].entity_id,
        Some(Uuid::from_u128(0x38).to_string())
    );
    assert_eq!(commit_events[0].entity_revision, Some(1));
}

#[tokio::test]
async fn failure_before_transaction_does_not_publish_a_workspace_refresh_event() {
    let (registry, candidate_id, lease, prepare, commit) =
        commit_fixture(CommitOutcome::BeforeTransaction("COMMIT_FAILED"));

    let events = run_worker(registry.clone(), lease, prepare, commit).await;

    assert_eq!(
        status(&registry, &candidate_id)["status"],
        "failed_before_commit"
    );
    assert!(environment_commit_events(&events).is_empty());
}

#[tokio::test]
async fn rolled_back_commit_does_not_publish_a_workspace_refresh_event() {
    let (registry, candidate_id, lease, prepare, commit) = commit_fixture(
        CommitOutcome::RolledBack(EnvironmentCommitRollbackOutcome::BaselineMismatch),
    );

    let events = run_worker(registry.clone(), lease, prepare, commit).await;

    assert_eq!(status(&registry, &candidate_id)["status"], "rolled_back");
    assert!(environment_commit_events(&events).is_empty());
}
