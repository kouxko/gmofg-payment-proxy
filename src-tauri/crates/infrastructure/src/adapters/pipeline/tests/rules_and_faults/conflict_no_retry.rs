#[derive(Debug)]
struct InvalidThenValidSnapshotRules {
    snapshot: Mutex<RuleRuntimeSnapshot>,
    snapshot_reads: AtomicUsize,
    commit_attempts: AtomicUsize,
}

#[async_trait]
impl RuntimeRuleRepository for InvalidThenValidSnapshotRules {
    async fn runtime_snapshot(&self, _channel: &ChannelId) -> AppResult<RuleRuntimeSnapshot> {
        let mut snapshot = self.snapshot.lock().clone();
        if self.snapshot_reads.fetch_add(1, AtomicOrdering::AcqRel) == 0 {
            snapshot.signature = intercept_proxy_domain::RuleSetSignature::from_rules(&[]);
        }
        Ok(snapshot)
    }

    async fn commit_runtime_deltas(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        deltas: &[intercept_proxy_domain::RuleLifecycleDelta],
    ) -> AppResult<u64> {
        self.commit_attempts.fetch_add(1, AtomicOrdering::AcqRel);
        let mut current = self.snapshot.lock();
        let revision = current.collection_revision + 1;
        *current = RuleRuntimeSnapshot::with_collection_identity(
            snapshot.collection_id,
            revision,
            crate::adapters::rules::conversion::apply_runtime_deltas(snapshot, deltas)?,
        );
        Ok(revision)
    }

    async fn reset_runtime_hit_metadata(&self, _collection_id: Uuid) -> AppResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn actor_validation_failure_restores_nth_checkpoint_before_next_evaluation() {
    let rule = view_to_domain_rule({
        let mut view = one_shot_delay_rule();
        view.draft.conditions =
            vec![intercept_proxy_application::RuleCondition::NthHit { count: 2 }];
        view.draft.one_shot = false;
        view
    })
    .expect("rule");
    let rules = Arc::new(InvalidThenValidSnapshotRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        snapshot_reads: AtomicUsize::new(0),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(128)),
        test_capture_repository(),
    );
    let epoch = Uuid::new_v4();
    pipeline.runtime_started(epoch).await;
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    let message = request_message(r#"{"amount":100}"#);
    let original = message.clone();

    let error = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            test_body_codec(),
        )
        .await
        .expect_err("invalid runtime snapshot must fail before commit");
    assert_eq!(error.code, "REVISION_CONFLICT");
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 0);
    assert_eq!(message.body, original.body);
    assert_eq!(message.headers, original.headers);

    let second = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            test_body_codec(),
        )
        .await
        .expect("validation failure must not consume the first Nth attempt");
    assert!(second.actions.is_empty());
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 1);
    assert_eq!(rules.snapshot.lock().rules[0].hit_count, 0);

    let third = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            test_body_codec(),
        )
        .await
        .expect("second committed Nth attempt matches");
    assert!(!third.actions.is_empty());
    assert_eq!(rules.snapshot.lock().rules[0].hit_count, 1);
}

#[tokio::test]
async fn nth_hit_conflict_is_not_retried_or_consumed() {
    let rule = view_to_domain_rule({
        let mut view = one_shot_delay_rule();
        view.draft.conditions =
            vec![intercept_proxy_application::RuleCondition::NthHit { count: 2 }];
        view.draft.one_shot = false;
        view
    })
    .expect("rule");
    let rules = Arc::new(ConflictOnceRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        conflict_once: AtomicBool::new(true),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(128)),
        test_capture_repository(),
    );
    let epoch = Uuid::new_v4();
    pipeline.runtime_started(epoch).await;
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    let message = request_message(r#"{"amount":100}"#);
    let body_codec = test_body_codec();

    let error = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            body_codec.clone(),
        )
        .await
        .expect_err("first no-match advance exposes the single commit conflict");
    assert_eq!(error.code, "REVISION_CONFLICT");
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 1);

    let second = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            body_codec.clone(),
        )
        .await
        .expect("second evaluation restarts from the unconsumed counter");
    assert!(second.actions.is_empty());
    assert_eq!(
        rules.commit_attempts.load(AtomicOrdering::Acquire),
        2,
        "a conflict is never retried"
    );
    {
        let persisted = rules.snapshot.lock();
        assert_eq!(persisted.rules[0].hit_count, 0);
        assert_eq!(
            persisted.collection_revision, 2,
            "the external conflict and successful no-match Nth advance are both visible"
        );
    }

    let third = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            body_codec.clone(),
        )
        .await
        .expect("third evaluation is the second committed attempt");
    assert!(!third.actions.is_empty(), "NthHit(2) now matches");
    assert_eq!(
        rules.snapshot.lock().rules[0].hit_count,
        1,
        "only the successful NthHit match increments hit metadata"
    );
}

#[tokio::test]
async fn nth_hit_actor_isolates_attempts_by_terminal_ip_and_certificate() {
    let rule = view_to_domain_rule({
        let mut view = one_shot_delay_rule();
        view.draft.conditions =
            vec![intercept_proxy_application::RuleCondition::NthHit { count: 2 }];
        view.draft.one_shot = false;
        view
    })
    .expect("rule");
    let rules = Arc::new(StaticRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
    });
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules,
        Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(128)),
        test_capture_repository(),
    );
    let epoch = Uuid::new_v4();
    pipeline.runtime_started(epoch).await;
    let base = test_context(epoch, Uuid::new_v4(), transaction_channel());
    let mut other_ip = base.clone();
    other_ip.peer_addr = "10.0.0.3:12345".parse().unwrap();
    let mut other_certificate = base.clone();
    other_certificate.tls_peer.as_mut().unwrap().sha256_fingerprint = "11:22:33".into();
    let message = request_message(r#"{"amount":100}"#);

    for context in [&base, &other_ip, &other_certificate] {
        let output = pipeline
            .evaluate(
                context,
                DomainMessageStage::Request,
                Some(&message),
                test_body_codec(),
            )
            .await
            .expect("isolated first attempt commits");
        assert!(output.actions.is_empty());
    }

    let second = pipeline
        .evaluate(
            &base,
            DomainMessageStage::Request,
            Some(&message),
            test_body_codec(),
        )
        .await
        .expect("same terminal second attempt");
    assert!(!second.actions.is_empty());
}
