#[derive(Debug)]
struct SocketTransactionRules {
    snapshot: Mutex<RuleRuntimeSnapshot>,
    commit_attempts: AtomicUsize,
    commit_guard: Option<Arc<std::sync::atomic::AtomicBool>>,
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
        if let Some(guard) = &self.commit_guard {
            assert!(
                guard.load(std::sync::atomic::Ordering::Acquire),
                "HTTP joint transaction must remain alive through lifecycle commit"
            );
        }
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
    outcome: TestSocketEncodeOutcome,
    original: Arc<Mutex<Vec<u8>>>,
}

enum TestSocketEncodeOutcome {
    Failure,
    Output(Vec<u8>),
}

#[async_trait]
impl intercept_proxy_runtime::SocketJointEvaluation for TestSocketJointEvaluation {
    fn gate(
        &mut self,
        _rule_id: Uuid,
    ) -> intercept_proxy_runtime::Result<intercept_proxy_runtime::JointRuleConditionEvaluation>
    {
        Ok(
            intercept_proxy_runtime::JointRuleConditionEvaluation::UnifiedOwned(
                intercept_proxy_runtime::JointConditionEvaluation { matched: true },
            ),
        )
    }

    async fn encode(
        self: Box<Self>,
    ) -> Result<intercept_proxy_exchange::SocketContext, intercept_proxy_exchange::Error> {
        assert_eq!(&*self.original.lock(), b"original-frame");
        match self.outcome {
            TestSocketEncodeOutcome::Failure => Err(intercept_proxy_exchange::Error::coded(
                "ENCODE_FAILED",
                "Rules: misleading text must not select the stage",
            )
            .with_external_package_call(
                intercept_proxy_exchange::ExternalPackageCallFailure {
                    package: intercept_proxy_domain::ProtocolPackageRef {
                        id: intercept_proxy_domain::ProtocolPackageId::new("socket-transaction")
                            .expect("package id"),
                        version: intercept_proxy_domain::ProtocolPackageVersion::new("1.0.0")
                            .expect("package version"),
                    },
                    direction: intercept_proxy_domain::ProtocolDirection::Upstream,
                    stage: intercept_proxy_exchange::ExternalPackageCallStage::Encode,
                    method: "hooks.encode".into(),
                    request_id: Some("socket-encode-red".into()),
                    remote_code: Some(-1),
                    stable_code: Some("ENCODE_FAILED".into()),
                    remote_message: Some("phase11 encode rejected".into()),
                    remote_data_summary: None,
                },
            )),
            TestSocketEncodeOutcome::Output(data) => {
                Ok(intercept_proxy_exchange::SocketContext { data })
            }
        }
    }
}

fn bounded_socket_evaluation(
    outcome: TestSocketEncodeOutcome,
    original: &Arc<Mutex<Vec<u8>>>,
    max_output_bytes: usize,
) -> Box<dyn intercept_proxy_runtime::SocketJointEvaluation> {
    Box::new(intercept_proxy_runtime::BoundedSocketJointEvaluation::<
        intercept_proxy_exchange::Upstream,
    >::new(
        Box::new(TestSocketJointEvaluation {
            outcome,
            original: Arc::clone(original),
        }),
        intercept_proxy_runtime::SocketPipelineLimits::new(1_024, max_output_bytes, 64)
            .expect("limits"),
    ))
}

fn socket_transaction_rule() -> intercept_proxy_domain::RuleDefinition {
    intercept_proxy_domain::RuleDefinition::create(
        intercept_proxy_domain::RuleDefinitionDraft {
            name: "socket transaction".into(),
            enabled: true,
            priority: 1,
            listener_id: intercept_proxy_domain::ListenerId::from_uuid(Uuid::from_u128(0x7472)),
            stage: intercept_proxy_domain::RuleStage::ProxyToUpstream,
            content: intercept_proxy_domain::RuleContent::Socket(
                intercept_proxy_domain::SocketRuleContent {
                    package: intercept_proxy_domain::ProtocolPackageRef {
                        id: intercept_proxy_domain::ProtocolPackageId::new("socket-transaction")
                            .expect("package id"),
                        version: intercept_proxy_domain::ProtocolPackageVersion::new("1.0.0")
                            .expect("package version"),
                    },
                    condition: intercept_proxy_domain::Condition::Document {
                        path: intercept_proxy_domain::JsonPointer::parse("").unwrap(),
                        predicate: intercept_proxy_domain::DocumentPredicate::NullEqual,
                    },
                    action: intercept_proxy_domain::UnifiedAction::RecordMatch,
                },
            ),
        },
        1,
    )
    .expect("rule")
}

#[tokio::test]
async fn socket_encode_failure_rolls_back_lifecycle_before_successful_commit() {
    let rule = socket_transaction_rule();
    let rules = Arc::new(SocketTransactionRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        commit_attempts: AtomicUsize::new(0),
        commit_guard: None,
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
    let original = Arc::new(Mutex::new(b"original-frame".to_vec()));

    let error = pipeline
        .apply_socket_policy(
            &context,
            intercept_proxy_runtime::SocketPayloadDirection::AppToUpstream,
            bounded_socket_evaluation(TestSocketEncodeOutcome::Failure, &original, 7),
        )
        .await
        .expect_err("Encode failure must fail before lifecycle commit");
    assert_eq!(error.code, "EXTERNAL_PACKAGE_CALL_FAILED");
    assert_eq!(error.exchange_code.as_deref(), Some("ENCODE_FAILED"));
    assert_eq!(
        error.stage,
        Some(intercept_proxy_exchange::ErrorStage::Encode)
    );
    assert_eq!(
        error.direction,
        Some(intercept_proxy_exchange::DirectionKind::Upstream)
    );
    assert_eq!(
        error
            .external_package_call
            .as_ref()
            .and_then(|failure| failure.stable_code.as_deref()),
        Some("ENCODE_FAILED")
    );
    assert_eq!(&*original.lock(), b"original-frame");
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 0);
    assert!(rules.snapshot.lock().rules[0].enabled());
    assert_eq!(rules.snapshot.lock().rules[0].lifecycle().hit_count, 0);

    for (output, expected_code) in [
        (Vec::new(), "EMPTY_OUTPUT"),
        (b"oversized".to_vec(), "OUTPUT_LIMIT_EXCEEDED"),
    ] {
        let error = pipeline
            .apply_socket_policy(
                &context,
                intercept_proxy_runtime::SocketPayloadDirection::AppToUpstream,
                bounded_socket_evaluation(TestSocketEncodeOutcome::Output(output), &original, 7),
            )
            .await
            .expect_err("invalid output must fail before lifecycle commit");
        assert_eq!(error.code, "EXTERNAL_PACKAGE_CALL_FAILED");
        assert_eq!(error.exchange_code.as_deref(), Some(expected_code));
        assert_eq!(
            error.stage,
            Some(intercept_proxy_exchange::ErrorStage::Encode)
        );
        assert_eq!(
            error.direction,
            Some(intercept_proxy_exchange::DirectionKind::Upstream)
        );
        assert_eq!(&*original.lock(), b"original-frame");
        assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 0);
        assert!(rules.snapshot.lock().rules[0].enabled());
        assert_eq!(rules.snapshot.lock().rules[0].lifecycle().hit_count, 0);
    }

    let encoded = pipeline
        .apply_socket_policy(
            &context,
            intercept_proxy_runtime::SocketPayloadDirection::AppToUpstream,
            bounded_socket_evaluation(
                TestSocketEncodeOutcome::Output(b"encoded".to_vec()),
                &original,
                7,
            ),
        )
        .await
        .expect("successful Encode commits the joint lifecycle transaction");
    assert_eq!(encoded.data, b"encoded");
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 1);
    assert!(rules.snapshot.lock().rules[0].enabled());
    assert_eq!(rules.snapshot.lock().rules[0].lifecycle().hit_count, 1);
}
