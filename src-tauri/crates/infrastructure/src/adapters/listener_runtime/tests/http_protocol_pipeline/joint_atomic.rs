#[tokio::test]
async fn non_utf8_encode_failure_keeps_one_shot_runtime_metadata_unchanged() {
    let listener = http_listener();
    let rule = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(23)),
        ProtocolRuleStage::AppToProxy,
        1,
        "route",
        Vec::new(),
        "changed",
    );
    let (snapshot, mut workspace) = snapshot(NON_UTF8_ENCODE_SCRIPT, &listener, vec![rule]);
    let definition = &mut workspace.rule_definitions[0];
    let mut draft = definition.to_draft();
    let RuleContent::Http(content) = &mut draft.content else {
        panic!("HTTP rule expected");
    };
    content.one_shot = true;
    definition.update(definition.revision(), draft).unwrap();
    let runtime_rules = workspace.http_runtime_rules().unwrap();
    let original_rule_revision = runtime_rules[0].revision;
    let collection_id = Uuid::from_u128(24);
    let repository = Arc::new(JointAtomicRules {
        snapshot: Mutex::new(
            intercept_proxy_domain::RuleRuntimeSnapshot::with_collection_identity_and_order(
                Some(collection_id),
                7,
                runtime_rules,
                workspace.http_runtime_rule_execution_order(),
            ),
        ),
    });
    let sessions = Arc::new(InMemorySessionStore::default());
    let pipeline = RuntimePipelineAdapter::new(
        RuntimePipelineProductHooks {
            body_codec: Arc::new(JointUtf8Codec),
            request_classifier: Arc::new(JointRequestClassifier),
            channel_labels: BTreeMap::new(),
        },
        repository.clone(),
        sessions.clone(),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(16)),
        Arc::new(CaptureRepositoryAdapter::new(sessions)),
    )
    .with_joint_http_rules(snapshot.joint_runtime());
    let identity = identity();
    let connection_context = ConnectionContext {
        runtime_epoch: identity.runtime_epoch,
        connection_id: identity.connection_id,
        channel: intercept_proxy_runtime::ChannelId::new(
            snapshot.observation_metadata().listener_id,
        )
        .unwrap(),
        peer_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        accepted_at: SystemTime::now(),
        tls_peer: None,
    };
    pipeline
        .runtime_started(connection_context.runtime_epoch)
        .await;
    let mut capabilities = snapshot.create_upstream(identity).unwrap();
    let original = context("POST / HTTP/1.1\r\n\r\n", "wire");
    let document = capabilities.decode.decode(&original).await.unwrap();
    capabilities.rules.apply(document).await.unwrap();
    let mut message = Message::from_raw_http1_head(
        original.header.as_bytes(),
        Bytes::copy_from_slice(original.body.as_bytes()),
    )
    .unwrap();
    let original_message = message.clone();
    let before = repository.snapshot.lock().clone();

    let error = pipeline
        .apply_request_policy(&connection_context, &mut message)
        .await
        .expect_err("non UTF-8 protocol output must fail before commit");

    assert_eq!(error.code, "INTERNAL_ERROR");
    assert_eq!(message.body, original_message.body);
    assert_eq!(message.headers, original_message.headers);
    let after = repository.snapshot.lock();
    assert_eq!(after.collection_revision, before.collection_revision);
    assert_eq!(after.rules[0].revision, original_rule_revision);
    assert!(after.rules[0].enabled);
    assert_eq!(after.rules[0].hit_count, 0);
    assert_eq!(after.rules[0].last_hit_at, None);
}
#[tokio::test]
async fn joint_document_state_is_isolated_by_connection_and_cleanup() {
    let listener = http_listener();
    let rule = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(25)),
        ProtocolRuleStage::AppToProxy,
        1,
        "route",
        Vec::new(),
        "changed",
    );
    let (snapshot, workspace) = snapshot(PIPELINE_SCRIPT, &listener, vec![rule]);
    let identity_a = identity();
    let identity_b = HttpConnectionIdentity {
        connection_id: Uuid::from_u128(26),
        ..identity_a.clone()
    };
    for (identity, body) in [(&identity_a, "alpha"), (&identity_b, "beta")] {
        let mut capabilities = snapshot.create_upstream(identity.clone()).unwrap();
        let document = capabilities
            .decode
            .decode(&context("POST / HTTP/1.1\r\n\r\n", body))
            .await
            .unwrap();
        capabilities.rules.apply(document).await.unwrap();
    }

    let written_b = execute_joint(
        &snapshot,
        &workspace,
        &identity_b,
        false,
        &context("POST / HTTP/1.1\r\n\r\n", "beta"),
    )
    .await
    .unwrap()
    .0;
    let written_a = execute_joint(
        &snapshot,
        &workspace,
        &identity_a,
        false,
        &context("POST / HTTP/1.1\r\n\r\n", "alpha"),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(written_b.body, Bytes::from_static(b"beta|changed"));
    assert_eq!(written_a.body, Bytes::from_static(b"alpha|changed"));

    for (identity, body) in [(&identity_a, "cleanup-a"), (&identity_b, "cleanup-b")] {
        let mut capabilities = snapshot.create_upstream(identity.clone()).unwrap();
        let document = capabilities
            .decode
            .decode(&context("POST / HTTP/1.1\r\n\r\n", body))
            .await
            .unwrap();
        capabilities.rules.apply(document).await.unwrap();
    }
    snapshot
        .joint_runtime()
        .remove_connection(&connection_context(&snapshot, &identity_a));
    assert!(snapshot.take_joint_evaluation(&identity_a, false).is_none());
    assert!(snapshot.take_joint_evaluation(&identity_b, false).is_some());
}

#[tokio::test]
async fn revision_conflict_retries_joint_document_from_checkpoint() {
    let listener = http_listener();
    let rule = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(27)),
        ProtocolRuleStage::AppToProxy,
        1,
        "route",
        Vec::new(),
        "changed",
    );
    let (snapshot, workspace) = snapshot(PIPELINE_SCRIPT, &listener, vec![rule]);
    let runtime_rules = workspace.http_runtime_rules().unwrap();
    let repository = Arc::new(ConflictJointRules {
        snapshot: Mutex::new(
            intercept_proxy_domain::RuleRuntimeSnapshot::with_collection_identity_and_order(
                Some(Uuid::from_u128(28)),
                7,
                runtime_rules,
                workspace.http_runtime_rule_execution_order(),
            ),
        ),
        conflict_once: AtomicBool::new(true),
        commit_attempts: AtomicUsize::new(0),
    });
    let sessions = Arc::new(InMemorySessionStore::default());
    let pipeline = RuntimePipelineAdapter::new(
        RuntimePipelineProductHooks {
            body_codec: Arc::new(JointUtf8Codec),
            request_classifier: Arc::new(JointRequestClassifier),
            channel_labels: BTreeMap::new(),
        },
        repository.clone(),
        sessions.clone(),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(16)),
        Arc::new(CaptureRepositoryAdapter::new(sessions)),
    )
    .with_joint_http_rules(snapshot.joint_runtime());
    let identity = identity();
    let connection_context = connection_context(&snapshot, &identity);
    pipeline
        .runtime_started(connection_context.runtime_epoch)
        .await;
    pipeline.connection_opened(&connection_context).await;
    let mut capabilities = snapshot.create_upstream(identity).unwrap();
    let original = context("POST / HTTP/1.1\r\n\r\n", "wire");
    let document = capabilities.decode.decode(&original).await.unwrap();
    capabilities.rules.apply(document).await.unwrap();
    let mut message = Message::from_raw_http1_head(
        original.header.as_bytes(),
        Bytes::copy_from_slice(original.body.as_bytes()),
    )
    .unwrap();

    pipeline
        .apply_request_policy(&connection_context, &mut message)
        .await
        .expect("retry succeeds");

    assert_eq!(message.body, Bytes::from_static(b"wire|changed"));
    assert_eq!(repository.commit_attempts.load(Ordering::Acquire), 2);
    let persisted = repository.snapshot.lock();
    assert_eq!(persisted.rules[0].hit_count, 1);
    assert_eq!(persisted.collection_revision, 9);
}

fn connection_context(
    snapshot: &HttpProtocolRuntimeSnapshot,
    identity: &HttpConnectionIdentity,
) -> ConnectionContext {
    ConnectionContext {
        runtime_epoch: identity.runtime_epoch,
        connection_id: identity.connection_id,
        channel: intercept_proxy_runtime::ChannelId::new(
            snapshot.observation_metadata().listener_id,
        )
        .unwrap(),
        peer_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        accepted_at: SystemTime::now(),
        tls_peer: None,
    }
}

#[derive(Debug)]
struct JointAtomicRules {
    snapshot: Mutex<intercept_proxy_domain::RuleRuntimeSnapshot>,
}

#[derive(Debug)]
struct ConflictJointRules {
    snapshot: Mutex<intercept_proxy_domain::RuleRuntimeSnapshot>,
    conflict_once: AtomicBool,
    commit_attempts: AtomicUsize,
}

#[async_trait]
impl RuntimeRuleRepository for ConflictJointRules {
    async fn runtime_snapshot(
        &self,
        _channel: &intercept_proxy_runtime::ChannelId,
    ) -> AppResult<intercept_proxy_domain::RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    async fn commit_runtime_snapshot(
        &self,
        snapshot: &intercept_proxy_domain::RuleRuntimeSnapshot,
        evaluated_rules: &[intercept_proxy_domain::Rule],
    ) -> AppResult<u64> {
        self.commit_attempts.fetch_add(1, Ordering::AcqRel);
        let mut current = self.snapshot.lock();
        if self.conflict_once.swap(false, Ordering::AcqRel) {
            let next = current.collection_revision + 1;
            *current =
                intercept_proxy_domain::RuleRuntimeSnapshot::with_collection_identity_and_order(
                    current.collection_id,
                    next,
                    current.rules.clone(),
                    current.execution_order.clone(),
                );
            return Err(AppError::new("REVISION_CONFLICT", "injected conflict"));
        }
        let next = current.collection_revision + 1;
        *current = intercept_proxy_domain::RuleRuntimeSnapshot::with_collection_identity_and_order(
            snapshot.collection_id,
            next,
            evaluated_rules.to_vec(),
            snapshot.execution_order.clone(),
        );
        Ok(next)
    }

    async fn reset_runtime_hit_metadata(&self, _collection_id: Uuid) -> AppResult<()> {
        Ok(())
    }
}

#[async_trait]
impl RuntimeRuleRepository for JointAtomicRules {
    async fn runtime_snapshot(
        &self,
        _channel: &intercept_proxy_runtime::ChannelId,
    ) -> AppResult<intercept_proxy_domain::RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    async fn commit_runtime_snapshot(
        &self,
        snapshot: &intercept_proxy_domain::RuleRuntimeSnapshot,
        evaluated_rules: &[intercept_proxy_domain::Rule],
    ) -> AppResult<u64> {
        let mut current = self.snapshot.lock();
        let next = current.collection_revision + 1;
        *current = intercept_proxy_domain::RuleRuntimeSnapshot::with_collection_identity_and_order(
            snapshot.collection_id,
            next,
            evaluated_rules.to_vec(),
            snapshot.execution_order.clone(),
        );
        Ok(next)
    }

    async fn reset_runtime_hit_metadata(&self, _collection_id: Uuid) -> AppResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct JointUtf8Codec;

impl BodyCodec for JointUtf8Codec {
    fn id(&self) -> &'static str {
        "joint-test"
    }
    fn name(&self) -> &'static str {
        "Joint test"
    }
    fn decode(&self, bytes: &[u8]) -> Result<String, intercept_proxy_product_api::ProductError> {
        String::from_utf8(bytes.to_vec()).map_err(|error| {
            intercept_proxy_product_api::ProductError::new("BODY_DECODE_FAILED", error.to_string())
        })
    }
    fn encode(&self, text: &str) -> Result<Vec<u8>, intercept_proxy_product_api::ProductError> {
        Ok(text.as_bytes().to_vec())
    }
}

#[derive(Debug)]
struct JointRequestClassifier;

impl RequestClassifier for JointRequestClassifier {
    fn classify(&self, _message: ProductMessageContext<'_>) -> ClassifiedRequest {
        ClassifiedRequest {
            request_id: None,
            request_type: None,
        }
    }
}
