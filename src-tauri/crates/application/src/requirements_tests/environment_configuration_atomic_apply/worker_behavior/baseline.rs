use super::*;

#[tokio::test]
async fn worker_uses_the_complete_candidate_frozen_baseline_instead_of_defaults() {
    let (registry, id) = queued_registry();
    let observed = EnvironmentApplyGenerations {
        package: 7,
        certificate_inventory: 11,
        protected_secret_inventory: 13,
        ..Default::default()
    };
    let lease = Arc::new(FakeLease::new(LeaseOutcome::Acquired(observed)));
    let prepare = Arc::new(FakePrepare {
        outcome: PrepareOutcome::Failure,
        calls: AtomicUsize::new(0),
    });
    let commit = Arc::new(FakeCommit {
        outcome: CommitOutcome::Success,
        calls: AtomicUsize::new(0),
        baselines: Mutex::new(Vec::new()),
    });

    run_worker(registry, lease.clone(), prepare, commit).await;

    let requests = lease.requests.lock().unwrap();
    assert_eq!(requests[0].candidate_id, id);
    assert_eq!(
        requests[0].candidate_epoch,
        EnvironmentCandidateEpoch::new(91)
    );
    assert_ne!(requests[0].expected, EnvironmentApplyGenerations::default());
    assert_eq!(
        requests[0].expected,
        *requests[0].validated_baseline.generations()
    );
    assert!(
        !requests[0]
            .validated_baseline
            .affected_listeners()
            .is_empty()
    );
    assert!(
        requests[0]
            .validated_baseline
            .exact_packages()
            .iter()
            .any(|package| package.package_id() == "au-eftex" && package.version() == "1.1.0")
    );
    assert!(
        !requests[0]
            .validated_baseline
            .material_inventory()
            .is_empty()
    );
    assert!(
        requests[0]
            .validated_baseline
            .affected_listeners()
            .windows(2)
            .all(|pair| pair[0].listener_id().to_string() <= pair[1].listener_id().to_string())
    );
    assert!(
        requests[0]
            .validated_baseline
            .exact_packages()
            .windows(2)
            .all(|pair| (pair[0].package_id(), pair[0].version())
                <= (pair[1].package_id(), pair[1].version()))
    );
}
