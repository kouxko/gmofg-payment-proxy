struct TestHttpJointEvaluation {
    fail_finalize: bool,
    active: Arc<std::sync::atomic::AtomicBool>,
    encoded: bool,
    finalize_calls: Arc<AtomicUsize>,
    gate_matches: bool,
    encode_body: Option<&'static [u8]>,
    expected_finalize_body: &'static [u8],
}

impl Drop for TestHttpJointEvaluation {
    fn drop(&mut self) {
        self.active
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

#[async_trait]
impl intercept_proxy_runtime::HttpJointEvaluation for TestHttpJointEvaluation {
    fn gate(
        &mut self,
        _rule_id: Uuid,
        _context: &intercept_proxy_domain::MatchContext<'_>,
        message: &mut Message,
        apply_action: &mut dyn FnMut(
            &mut Message,
            &intercept_proxy_domain::HttpAction,
        ) -> intercept_proxy_runtime::Result<()>,
    ) -> intercept_proxy_runtime::Result<intercept_proxy_runtime::JointRuleConditionEvaluation>
    {
        if self.gate_matches {
            apply_action(
                message,
                &intercept_proxy_domain::HttpAction::ReplaceBodyText("action-body".into()),
            )?;
        }
        Ok(
            intercept_proxy_runtime::JointRuleConditionEvaluation::UnifiedOwned(
                intercept_proxy_runtime::JointConditionEvaluation {
                    matched: self.gate_matches,
                },
            ),
        )
    }

    async fn encode(
        &mut self,
        message: &mut Message,
    ) -> Result<(), intercept_proxy_exchange::Error> {
        self.encoded = true;
        if let Some(body) = self.encode_body {
            message.replace_body(bytes::Bytes::from_static(body));
        }
        Ok(())
    }
}

#[async_trait]
impl intercept_proxy_runtime::Finalize for TestHttpJointEvaluation {
    async fn finalize(
        &mut self,
        request: &HttpRequestMetadata,
        message: &mut Message,
    ) -> Result<(), intercept_proxy_exchange::Error> {
        assert_eq!(request.url, "/payment");
        assert!(self.encoded, "Finalize must run after Encode");
        assert_eq!(
            &message.body[..],
            self.expected_finalize_body,
            "Finalize must observe the body after HTTP actions"
        );
        self.finalize_calls.fetch_add(1, AtomicOrdering::AcqRel);
        if self.fail_finalize {
            return Err(intercept_proxy_exchange::Error::coded(
                "FINALIZE_FAILED",
                "Finalize rejected the encoded HTTP message",
            )
            .with_external_package_call(
                intercept_proxy_exchange::ExternalPackageCallFailure {
                    package: intercept_proxy_domain::ProtocolPackageRef {
                        id: intercept_proxy_domain::ProtocolPackageId::new("http-finalize")
                            .unwrap(),
                        version: intercept_proxy_domain::ProtocolPackageVersion::new("1.0.0")
                            .unwrap(),
                    },
                    direction: intercept_proxy_domain::ProtocolDirection::Upstream,
                    stage: intercept_proxy_exchange::ExternalPackageCallStage::Finalize,
                    method: "upstream-finalize".into(),
                    request_id: None,
                    remote_code: None,
                    stable_code: Some("FINALIZE_FAILED".into()),
                    remote_message: Some("Finalize rejected the encoded HTTP message".into()),
                    remote_data_summary: None,
                },
            ));
        }
        message.replace_body(bytes::Bytes::from_static(b"finalized"));
        Ok(())
    }
}

fn http_finalize_rule() -> RuleDefinition {
    RuleDefinition::create(
        intercept_proxy_domain::RuleDefinitionDraft {
            name: "HTTP Finalize transaction".into(),
            enabled: true,
            priority: 1,
            listener_id: intercept_proxy_domain::ListenerId::from_uuid(Uuid::from_u128(0x7472)),
            stage: intercept_proxy_domain::RuleStage::ProxyToUpstream,
            content: intercept_proxy_domain::RuleContent::Http(
                intercept_proxy_domain::HttpRuleContent {
                    description: String::new(),
                    condition: intercept_proxy_domain::Condition::Http {
                        field: intercept_proxy_domain::MatchField::Method,
                        operator: intercept_proxy_domain::MatchOperator::Equals("POST".into()),
                    },
                    action: intercept_proxy_domain::UnifiedAction::Http(
                        intercept_proxy_domain::HttpAction::ReplaceBodyText("action-body".into()),
                    ),
                },
            ),
        },
        1,
    )
    .unwrap()
}

#[tokio::test]
async fn http_finalize_runs_after_encode_and_before_lifecycle_commit() {
    let rules = Arc::new(SocketTransactionRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![http_finalize_rule()])),
        commit_attempts: AtomicUsize::new(0),
        commit_guard: Some(Arc::new(std::sync::atomic::AtomicBool::new(true))),
    });
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::default()),
        Arc::new(EventHub::new(16)),
        test_capture_repository(),
    );
    let context = test_context(Uuid::new_v4(), Uuid::new_v4(), transaction_channel());
    pipeline.runtime_started(context.runtime_epoch).await;
    let finalize_calls = Arc::new(AtomicUsize::new(0));

    let error = pipeline
        .rule_runtime
        .evaluate(
            &context,
            DomainMessageStage::Request,
            request_metadata(),
            Some(&request_message("original")),
            test_body_codec(),
            Some(Box::new(TestHttpJointEvaluation {
                fail_finalize: true,
                active: Arc::new(std::sync::atomic::AtomicBool::new(true)),
                encoded: false,
                finalize_calls: Arc::clone(&finalize_calls),
                gate_matches: true,
                encode_body: Some(b"encoded"),
                expected_finalize_body: b"encoded",
            })),
        )
        .await
        .expect_err("Finalize failure must precede lifecycle commit");
    assert_eq!(
        error.stage,
        Some(intercept_proxy_exchange::ErrorStage::Finalize)
    );
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 0);
    assert_eq!(rules.snapshot.lock().rules[0].lifecycle().hit_count, 0);

    let active = rules.commit_guard.as_ref().unwrap().clone();
    active.store(true, std::sync::atomic::Ordering::Release);
    let evaluated = pipeline
        .rule_runtime
        .evaluate(
            &context,
            DomainMessageStage::Request,
            request_metadata(),
            Some(&request_message("original")),
            test_body_codec(),
            Some(Box::new(TestHttpJointEvaluation {
                fail_finalize: false,
                active: Arc::clone(&active),
                encoded: false,
                finalize_calls: Arc::clone(&finalize_calls),
                gate_matches: true,
                encode_body: None,
                expected_finalize_body: b"action-body",
            })),
        )
        .await
        .unwrap();
    assert_eq!(&evaluated.prepared_message.unwrap().body[..], b"finalized");
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 1);
    assert_eq!(rules.snapshot.lock().rules[0].lifecycle().hit_count, 1);
    assert!(!active.load(std::sync::atomic::Ordering::Acquire));

    let unmatched = pipeline
        .rule_runtime
        .evaluate(
            &context,
            DomainMessageStage::Request,
            request_metadata(),
            Some(&request_message("original")),
            test_body_codec(),
            Some(Box::new(TestHttpJointEvaluation {
                fail_finalize: false,
                active: Arc::new(std::sync::atomic::AtomicBool::new(true)),
                encoded: false,
                finalize_calls: Arc::clone(&finalize_calls),
                gate_matches: false,
                encode_body: None,
                expected_finalize_body: b"original",
            })),
        )
        .await
        .unwrap();
    assert_eq!(&unmatched.prepared_message.unwrap().body[..], b"finalized");
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 1);
    assert_eq!(finalize_calls.load(AtomicOrdering::Acquire), 3);
}
