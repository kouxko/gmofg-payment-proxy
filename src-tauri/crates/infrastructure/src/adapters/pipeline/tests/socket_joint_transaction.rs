#[derive(Debug)]
struct SocketTransactionRules {
    snapshot: Mutex<RuleRuntimeSnapshot>,
    commit_attempts: AtomicUsize,
}

#[async_trait]
impl RuntimeRuleRepository for SocketTransactionRules {
    async fn runtime_snapshot(&self, _channel: &ChannelId) -> AppResult<RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    async fn commit_runtime_deltas(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        deltas: &[RuleLifecycleDelta],
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

struct TestSocketJointEvaluation {
    fail_encode: bool,
}

#[async_trait]
impl intercept_proxy_runtime::SocketJointEvaluation for TestSocketJointEvaluation {
    fn gate(
        &mut self,
        _rule_id: Uuid,
        _nth_attempt: u64,
    ) -> intercept_proxy_runtime::Result<intercept_proxy_runtime::JointRuleConditionEvaluation>
    {
        Ok(
            intercept_proxy_runtime::JointRuleConditionEvaluation::UnifiedOwned(
                intercept_proxy_runtime::JointConditionEvaluation {
                    matched: true,
                    eligible_without_nth: true,
                    contains_nth: false,
                },
            ),
        )
    }

    async fn encode(
        self: Box<Self>,
    ) -> Result<intercept_proxy_exchange::SocketContext, intercept_proxy_exchange::Error> {
        if self.fail_encode {
            Err(intercept_proxy_exchange::Error::new(
                "EXTERNAL_PACKAGE_CALL_FAILED: phase11 encode rejected",
            ))
        } else {
            Ok(intercept_proxy_exchange::SocketContext {
                data: b"encoded".to_vec(),
            })
        }
    }
}

#[tokio::test]
async fn socket_encode_failure_rolls_back_lifecycle_before_successful_commit() {
    let rule = intercept_proxy_domain::RuleDefinition::create(
        intercept_proxy_domain::RuleDefinitionDraft {
            name: "one-shot socket transaction".into(),
            enabled: true,
            priority: 1,
            listener_id: intercept_proxy_domain::ListenerId::from_uuid(Uuid::from_u128(0x7472)),
            stage: intercept_proxy_domain::RuleStage::ProxyToUpstream,
            one_shot: true,
            content: intercept_proxy_domain::RuleContent::Socket(
                intercept_proxy_domain::SocketRuleContent {
                    package: intercept_proxy_domain::ProtocolPackageRef {
                        id: intercept_proxy_domain::ProtocolPackageId::new("socket-transaction")
                            .expect("package id"),
                        version: intercept_proxy_domain::ProtocolPackageVersion::new("1.0.0")
                            .expect("package version"),
                    },
                    conditions: vec![intercept_proxy_domain::Condition::NthHit { count: 1 }],
                    actions: vec![intercept_proxy_domain::UnifiedAction::RecordMatch],
                },
            ),
        },
        1,
    )
    .expect("rule");
    let rules = Arc::new(SocketTransactionRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::default()),
        Arc::new(EventHub::new(16)),
        test_capture_repository(),
    );
    let epoch = Uuid::new_v4();
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    open_test_connection(&pipeline, &context).await;

    let error = pipeline
        .apply_socket_policy(
            &context,
            intercept_proxy_runtime::SocketPayloadDirection::AppToUpstream,
            Box::new(TestSocketJointEvaluation { fail_encode: true }),
        )
        .await
        .expect_err("Encode failure must fail before lifecycle commit");
    assert_eq!(error.code, "EXTERNAL_PACKAGE_CALL_FAILED");
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 0);
    assert!(rules.snapshot.lock().rules[0].enabled());
    assert_eq!(rules.snapshot.lock().rules[0].lifecycle().hit_count, 0);

    let encoded = pipeline
        .apply_socket_policy(
            &context,
            intercept_proxy_runtime::SocketPayloadDirection::AppToUpstream,
            Box::new(TestSocketJointEvaluation { fail_encode: false }),
        )
        .await
        .expect("successful Encode commits the joint lifecycle transaction");
    assert_eq!(encoded.data, b"encoded");
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 1);
    assert!(!rules.snapshot.lock().rules[0].enabled());
    assert_eq!(rules.snapshot.lock().rules[0].lifecycle().hit_count, 1);
}
