fn cancellation_one_shot_rule() -> RuleDefinition {
    RuleDefinition::create(
        intercept_proxy_domain::RuleDefinitionDraft {
            name: "cancellation one-shot".into(),
            enabled: true,
            priority: 1,
            listener_id: intercept_proxy_domain::ListenerId::from_uuid(Uuid::from_u128(0x7472)),
            stage: intercept_proxy_domain::RuleStage::ProxyToUpstream,
            one_shot: true,
            content: intercept_proxy_domain::RuleContent::Http(
                intercept_proxy_domain::HttpRuleContent {
                    description: String::new(),
                    conditions: vec![intercept_proxy_domain::Condition::NthHit { count: 1 }],
                    actions: vec![intercept_proxy_domain::UnifiedAction::RecordMatch],
                },
            ),
        },
        1,
    )
    .unwrap()
}

#[derive(Debug)]
struct BlockingCommitRules {
    snapshot: Mutex<RuleRuntimeSnapshot>,
    commit_entered: Arc<tokio::sync::Notify>,
    commit_release: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl RuntimeRuleRepository for BlockingCommitRules {
    async fn runtime_snapshot(&self, _: &ChannelId) -> AppResult<RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    async fn commit_runtime_deltas(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        deltas: &[intercept_proxy_domain::RuleLifecycleDelta],
    ) -> AppResult<u64> {
        self.commit_entered.notify_one();
        self.commit_release.notified().await;
        let mut current = self.snapshot.lock();
        if current.collection_revision != snapshot.collection_revision
            || current.signature != snapshot.signature
        {
            return Err(AppError::new("REVISION_CONFLICT", "规则测试快照已变化。"));
        }
        let revision = current.collection_revision.saturating_add(1);
        *current = RuleRuntimeSnapshot::with_collection_identity_and_order(
            snapshot.collection_id,
            revision,
            crate::adapters::rules::conversion::apply_runtime_deltas(snapshot, deltas)?,
            snapshot.execution_order.clone(),
        );
        Ok(revision)
    }

    async fn reset_runtime_hit_metadata(&self, _: Uuid) -> AppResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct BlockingStopRules {
    snapshot: Mutex<RuleRuntimeSnapshot>,
    reset_calls: std::sync::atomic::AtomicUsize,
    stop_reset_entered: Arc<tokio::sync::Notify>,
    stop_reset_release: Arc<tokio::sync::Notify>,
    stop_reset_completed: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl RuntimeRuleRepository for BlockingStopRules {
    async fn runtime_snapshot(&self, _: &ChannelId) -> AppResult<RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    async fn commit_runtime_deltas(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        deltas: &[intercept_proxy_domain::RuleLifecycleDelta],
    ) -> AppResult<u64> {
        let mut current = self.snapshot.lock();
        let revision = current.collection_revision.saturating_add(1);
        *current = RuleRuntimeSnapshot::with_collection_identity_and_order(
            snapshot.collection_id,
            revision,
            crate::adapters::rules::conversion::apply_runtime_deltas(snapshot, deltas)?,
            snapshot.execution_order.clone(),
        );
        Ok(revision)
    }

    async fn reset_runtime_hit_metadata(&self, _: Uuid) -> AppResult<()> {
        let call = self
            .reset_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if call > 0 {
            self.stop_reset_entered.notify_one();
            self.stop_reset_release.notified().await;
        }
        let mut current = self.snapshot.lock();
        current.rules = current
            .rules
            .iter()
            .map(reset_rule_lifecycle)
            .collect::<AppResult<Vec<_>>>()?;
        let revision = current.collection_revision.saturating_add(1);
        *current = RuleRuntimeSnapshot::with_collection_identity_and_order(
            current.collection_id,
            revision,
            current.rules.clone(),
            current.execution_order.clone(),
        );
        if call > 0 {
            self.stop_reset_completed.notify_one();
        }
        Ok(())
    }
}

#[tokio::test(flavor = "current_thread")]
async fn aborting_http_caller_after_commit_started_does_not_cancel_actor_state_machine() {
    let commit_entered = Arc::new(tokio::sync::Notify::new());
    let commit_release = Arc::new(tokio::sync::Notify::new());
    let rules = Arc::new(BlockingCommitRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![cancellation_one_shot_rule()])),
        commit_entered: Arc::clone(&commit_entered),
        commit_release: Arc::clone(&commit_release),
    });
    let pipeline = Arc::new(RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::default()),
        Arc::new(EventHub::new(16)),
        test_capture_repository(),
    ));
    let context = test_context(Uuid::new_v4(), Uuid::new_v4(), transaction_channel());
    pipeline.runtime_started(context.runtime_epoch).await;
    let message = request_message("body");
    let caller = tokio::spawn({
        let pipeline = pipeline.clone();
        let context = context.clone();
        let message = message.clone();
        async move {
            pipeline
                .rule_runtime
                .evaluate(
                    &context,
                    DomainMessageStage::Request,
                    request_metadata(),
                    Some(&message),
                    test_body_codec(),
                    None,
                )
                .await
        }
    });
    commit_entered.notified().await;

    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    commit_release.notify_one();
    pipeline
        .rule_runtime
        .evaluate(
            &context,
            DomainMessageStage::Request,
            request_metadata(),
            Some(&message),
            test_body_codec(),
            None,
        )
        .await
        .unwrap();

    let persisted = rules.snapshot.lock();
    assert!(!persisted.rules[0].enabled());
    assert_eq!(persisted.rules[0].lifecycle().hit_count, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn aborted_runtime_stopping_still_retires_epoch_and_resets_actor() {
    let collection_id = Uuid::new_v4();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let completed = Arc::new(tokio::sync::Notify::new());
    let rules = Arc::new(BlockingStopRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity(
            Some(collection_id),
            1,
            vec![cancellation_one_shot_rule()],
        )),
        reset_calls: std::sync::atomic::AtomicUsize::new(0),
        stop_reset_entered: Arc::clone(&entered),
        stop_reset_release: Arc::clone(&release),
        stop_reset_completed: Arc::clone(&completed),
    });
    let pipeline = Arc::new(RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::default()),
        Arc::new(EventHub::new(16)),
        test_capture_repository(),
    ));
    let epoch = Uuid::new_v4();
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    pipeline.runtime_started(epoch).await;
    pipeline
        .rule_runtime
        .evaluate(
            &context,
            DomainMessageStage::Request,
            request_metadata(),
            Some(&request_message("body")),
            test_body_codec(),
            None,
        )
        .await
        .unwrap();
    let stopping = tokio::spawn({
        let pipeline = pipeline.clone();
        async move { pipeline.rule_runtime.runtime_stopping(epoch).await }
    });
    entered.notified().await;

    stopping.abort();
    assert!(stopping.await.unwrap_err().is_cancelled());
    release.notify_one();
    completed.notified().await;

    assert_eq!(rules.snapshot.lock().rules[0].lifecycle().hit_count, 0);
    assert!(pipeline.rule_runtime.prepare_epoch(epoch).is_err());
}
