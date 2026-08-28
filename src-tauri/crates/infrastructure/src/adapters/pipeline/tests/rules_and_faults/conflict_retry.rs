#[tokio::test]
async fn nth_hit_retry_preserves_prior_count_without_double_counting_current_message() {
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

    let first = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            body_codec.clone(),
        )
        .await
        .expect("first evaluation");
    assert!(
        first.actions.is_empty(),
        "the first NthHit(2) evaluation only advances the in-memory counter"
    );
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 0);

    let second = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            body_codec.clone(),
        )
        .await
        .expect("second evaluation retries after the injected conflict");
    assert_eq!(
        second.actions,
        vec![RuleAction::Delay { milliseconds: 25 }],
        "the same second request must still hit after rollback and re-evaluation"
    );
    assert_eq!(
        rules.commit_attempts.load(AtomicOrdering::Acquire),
        2,
        "one conflicting CAS and one successful retry are expected"
    );
    {
        let persisted = rules.snapshot.lock();
        assert_eq!(persisted.rules[0].hit_count, 1);
        assert_eq!(
            persisted.collection_revision, 2,
            "the external advance and successful retry each advance once"
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
        .expect("third evaluation");
    assert!(
        third.actions.is_empty(),
        "the retry must not count the second request twice"
    );
    assert_eq!(
        rules.snapshot.lock().rules[0].hit_count,
        1,
        "only the exact second hit executes and persists"
    );
}
