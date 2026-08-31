use super::*;

pub(super) fn http_request_metadata() -> intercept_proxy_runtime::http::HttpRequestMetadata {
    intercept_proxy_runtime::http::HttpRequestMetadata {
        method: "POST".into(),
        request_target: "/phase10".into(),
    }
}

#[derive(Debug)]
struct FixedHttpProvider(RuntimeExternalSocketPackageBinding);

#[async_trait]
impl ExternalSocketPackageProvider for FixedHttpProvider {
    async fn resolve(
        &self,
        package: &ProtocolPackageRef,
    ) -> intercept_proxy_application::AppResult<Option<RuntimeExternalSocketPackageBinding>> {
        Ok((self.0.registration().package().identity() == *package).then(|| self.0.clone()))
    }
}

pub(super) async fn prepared_external_snapshot_for(
    rpc: Arc<RecordingHttpRpc>,
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
) -> Arc<HttpProtocolRuntimeSnapshot> {
    prepared_external_snapshot_for_registration(rpc, workspace, listener, http_registration()).await
}

pub(super) async fn prepared_external_snapshot_for_registration(
    rpc: Arc<RecordingHttpRpc>,
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    registration: PackageManifest,
) -> Arc<HttpProtocolRuntimeSnapshot> {
    let adapter = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    adapter.set_external_package_provider(Arc::new(FixedHttpProvider(
        RuntimeExternalSocketPackageBinding::new(registration, rpc),
    )));
    HttpProtocolRuntimeSnapshot::prepare_async(&adapter, workspace, listener)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn production_snapshot_compiles_recursive_or_with_insert_and_append() {
    use intercept_proxy_domain::{
        Condition, ConditionTree, DocumentMutation, DocumentPredicate, HttpDocumentRuleContent,
        RuleStage, StringOperator, StringPredicate, UnifiedAction,
    };

    let listener = phase10_listener();
    let condition = |expected: &str| {
        ConditionTree::Leaf(Condition::Document {
            path: JsonPointer::property("value"),
            predicate: DocumentPredicate::String(StringPredicate {
                operator: StringOperator::Equal,
                value: expected.to_owned(),
            }),
        })
    };
    let definition = RuleDefinition::create(
        RuleDefinitionDraft {
            name: "recursive OR insert append".into(),
            enabled: true,
            priority: 10,
            listener_id: listener.id,
            stage: RuleStage::ProxyToUpstream,
            one_shot: false,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition: ConditionTree::Any(vec![condition("old"), condition("fallback")]),
                actions: vec![
                    UnifiedAction::Document(DocumentMutation::Set {
                        path: JsonPointer::property("items"),
                        value: DocumentValue::Array(Vec::new()),
                    }),
                    UnifiedAction::Document(DocumentMutation::Insert {
                        path: JsonPointer::property("items"),
                        index: 0,
                        value: DocumentValue::String("first".into()),
                    }),
                    UnifiedAction::Document(DocumentMutation::Append {
                        path: JsonPointer::property("items"),
                        value: DocumentValue::String("last".into()),
                    }),
                ],
                document: Some(HttpDocumentRuleContent {
                    package: phase10_package(),
                }),
            }),
        },
        1,
    )
    .unwrap();
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        rule_definitions: vec![definition],
        rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };

    let runtime_rules = workspace.rule_definitions.clone();
    let identity = HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(991),
        connection_id: Uuid::from_u128(992),
        peer: "127.0.0.1:1992".into(),
    };
    assert_production_changed_commit(
        &listener,
        &workspace,
        runtime_rules.clone(),
        &identity,
        br#"{"items":["first","last"],"value":"old"}"#,
    )
    .await;
    assert_production_encode_failure_rolls_back(&listener, &workspace, runtime_rules, identity)
        .await;
}

#[tokio::test]
async fn production_http_actor_owns_unified_nth_attempt_and_one_shot_commit() {
    use intercept_proxy_domain::{
        Condition, ConditionTree, DocumentMutation, DocumentPredicate, HttpDocumentRuleContent,
        RuleStage, StringOperator, StringPredicate, UnifiedAction,
    };

    let listener = phase10_listener();
    let definition = RuleDefinition::create(
        RuleDefinitionDraft {
            name: "HTTP unified Nth".into(),
            enabled: true,
            priority: 10,
            listener_id: listener.id,
            stage: RuleStage::ProxyToUpstream,
            one_shot: true,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition: ConditionTree::All(vec![
                    ConditionTree::Any(vec![
                        ConditionTree::Leaf(Condition::Document {
                            path: JsonPointer::property("value"),
                            predicate: DocumentPredicate::String(StringPredicate {
                                operator: StringOperator::Equal,
                                value: "old".into(),
                            }),
                        }),
                        ConditionTree::Leaf(Condition::Document {
                            path: JsonPointer::property("value"),
                            predicate: DocumentPredicate::String(StringPredicate {
                                operator: StringOperator::Equal,
                                value: "fallback".into(),
                            }),
                        }),
                    ]),
                    ConditionTree::Leaf(Condition::NthHit { count: 2 }),
                ]),
                actions: vec![UnifiedAction::Document(DocumentMutation::Set {
                    path: JsonPointer::property("value"),
                    value: DocumentValue::String("new".into()),
                })],
                document: Some(HttpDocumentRuleContent {
                    package: phase10_package(),
                }),
            }),
        },
        1,
    )
    .unwrap();
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        rule_definitions: vec![definition],
        rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    let snapshot = prepared_external_snapshot_for(
        Arc::new(RecordingHttpRpc::new(false)),
        &workspace,
        &listener,
    )
    .await;
    let repository = Arc::new(Phase10ActorRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
            Some(Uuid::from_u128(993)),
            7,
            workspace.rule_definitions.clone(),
            workspace.http_runtime_rule_execution_order(),
        )),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = actor_pipeline(&snapshot, Arc::clone(&repository));
    pipeline.runtime_started(Uuid::from_u128(994)).await;

    for (index, expected) in [
        br#"{"value":"old"}"#.as_slice(),
        br#"{"value":"new"}"#.as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        let actual = execute_nth_http_attempt(&snapshot, &pipeline, index).await;
        assert_eq!(actual.body.as_ref(), expected);
    }

    assert_eq!(repository.commit_attempts.load(Ordering::SeqCst), 2);
    let committed = repository.snapshot.lock().clone();
    assert_eq!(committed.rules[0].lifecycle().hit_count, 1);
    assert!(!committed.rules[0].enabled());
}

pub(super) async fn execute_nth_http_attempt(
    snapshot: &HttpProtocolRuntimeSnapshot,
    pipeline: &RuntimePipelineAdapter,
    index: usize,
) -> Message {
    let identity = HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(994),
        connection_id: Uuid::from_u128(995 + index as u128),
        peer: "127.0.0.1:1902".into(),
    };
    let connection = actor_context(snapshot, &identity);
    pipeline.connection_opened(&connection).await;
    let mut capabilities = snapshot.create_upstream(identity).unwrap();
    let context = phase10_http_context();
    let document = capabilities.decode.decode(&context).await.unwrap();
    capabilities.rules.apply(document).await.unwrap();
    let mut message = Message::from_raw_http1_head(
        context.header.as_bytes(),
        Bytes::copy_from_slice(&context.wire_body),
    )
    .unwrap();
    pipeline
        .apply_request_policy(&connection, &http_request_metadata(), &mut message)
        .await
        .unwrap();
    message
}

#[derive(Debug)]
pub(super) struct Phase10ActorRules {
    pub(super) snapshot: Mutex<RuleRuntimeSnapshot>,
    pub(super) commit_attempts: AtomicUsize,
}

#[async_trait]
impl RuntimeRuleRepository for Phase10ActorRules {
    async fn runtime_snapshot(
        &self,
        _channel: &intercept_proxy_runtime::ChannelId,
    ) -> intercept_proxy_application::AppResult<RuleRuntimeSnapshot> {
        Ok(self.snapshot.lock().clone())
    }

    async fn commit_runtime_deltas(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        deltas: &[intercept_proxy_domain::RuleLifecycleDelta],
    ) -> intercept_proxy_application::AppResult<u64> {
        self.commit_attempts.fetch_add(1, Ordering::AcqRel);
        let mut current = self.snapshot.lock();
        let next = current.collection_revision + 1;
        *current = RuleRuntimeSnapshot::with_collection_identity_and_order(
            snapshot.collection_id,
            next,
            crate::adapters::rules::conversion::apply_runtime_deltas(snapshot, deltas)?,
            snapshot.execution_order.clone(),
        );
        Ok(next)
    }

    async fn reset_runtime_hit_metadata(
        &self,
        _collection_id: Uuid,
    ) -> intercept_proxy_application::AppResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct Phase10RequestClassifier;

impl RequestClassifier for Phase10RequestClassifier {
    fn classify(&self, _message: ProductMessageContext<'_>) -> ClassifiedRequest {
        ClassifiedRequest {
            request_id: None,
            request_type: None,
        }
    }
}

pub(super) fn actor_context(
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
        peer_addr: "127.0.0.1:1902".parse::<SocketAddr>().unwrap(),
        accepted_at: SystemTime::now(),
        tls_peer: None,
    }
}

pub(super) fn actor_pipeline(
    snapshot: &HttpProtocolRuntimeSnapshot,
    repository: Arc<Phase10ActorRules>,
) -> RuntimePipelineAdapter {
    let sessions = Arc::new(intercept_proxy_application::InMemorySessionStore::default());
    RuntimePipelineAdapter::new(
        RuntimePipelineProductHooks {
            body_codec: Arc::new(StrictTestUtf8),
            request_classifier: Arc::new(Phase10RequestClassifier),
            channel_labels: BTreeMap::new(),
        },
        repository,
        Arc::clone(&sessions),
        Arc::new(intercept_proxy_application::BreakpointCoordinator::default()),
        Arc::new(intercept_proxy_application::EventHub::new(16)),
        Arc::new(CaptureRepositoryAdapter::new(sessions)),
    )
    .with_joint_http_rules(snapshot.joint_runtime())
}

pub(super) fn phase10_http_context() -> HttpContext {
    HttpContext {
        header: "POST / HTTP/1.1\r\nContent-Type: application/json; charset=utf-8\r\n\r\n".into(),
        body: r#"{"value":"old"}"#.into(),
        body_is_utf8: true,
        wire_body: br#"{"value":"old"}"#.to_vec(),
    }
}

#[tokio::test]
async fn production_snapshot_uses_shared_provider_for_both_directions_and_joint_encode() {
    let listener = phase10_listener();
    let rule = set_string_rule(
        &listener,
        RuleId::from_uuid(Uuid::from_u128(903)),
        RuleStage::ProxyToUpstream,
        1,
        "value",
        vec![ConditionTree::Leaf(Condition::Document {
            path: JsonPointer::property("value"),
            predicate: DocumentPredicate::String(StringPredicate {
                operator: StringOperator::Equal,
                value: "old".into(),
            }),
        })],
        "new",
    );
    let workspace = workspace_with_http_rules(&listener, vec![rule]);
    let runtime_rules = workspace.rule_definitions.clone();
    let identity = HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(901),
        connection_id: Uuid::from_u128(902),
        peer: "127.0.0.1:1902".into(),
    };

    assert_production_changed_commit(
        &listener,
        &workspace,
        runtime_rules.clone(),
        &identity,
        br#"{"value":"new"}"#,
    )
    .await;
    assert_production_encode_failure_rolls_back(&listener, &workspace, runtime_rules, identity)
        .await;
}

pub(super) async fn assert_production_changed_commit(
    listener: &ProxyListener,
    workspace: &ProxyWorkspace,
    runtime_rules: Vec<intercept_proxy_domain::RuleDefinition>,
    identity: &HttpConnectionIdentity,
    expected_body: &'static [u8],
) {
    let rpc = Arc::new(RecordingHttpRpc::new(false));
    let snapshot = prepared_external_snapshot_for(rpc.clone(), workspace, listener).await;
    let repository = Arc::new(Phase10ActorRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
            Some(Uuid::from_u128(904)),
            7,
            runtime_rules.clone(),
            workspace.http_runtime_rule_execution_order(),
        )),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = actor_pipeline(&snapshot, Arc::clone(&repository));
    let connection = actor_context(&snapshot, identity);
    pipeline.runtime_started(connection.runtime_epoch).await;
    pipeline.connection_opened(&connection).await;
    let mut upstream = snapshot.create_upstream(identity.clone()).unwrap();
    snapshot.create_downstream(identity.clone()).unwrap();
    let context = phase10_http_context();
    let upstream_document = upstream.decode.decode(&context).await.unwrap();
    upstream.display.display(&upstream_document).await.unwrap();
    upstream.rules.apply(upstream_document).await.unwrap();
    let mut message = Message::from_raw_http1_head(
        context.header.as_bytes(),
        Bytes::copy_from_slice(&context.wire_body),
    )
    .unwrap();
    pipeline
        .apply_request_policy(&connection, &http_request_metadata(), &mut message)
        .await
        .unwrap();
    assert_eq!(message.body, Bytes::from_static(expected_body));
    assert_eq!(rpc.encode_calls.load(Ordering::SeqCst), 1);
    assert_eq!(repository.commit_attempts.load(Ordering::SeqCst), 1);
    let committed = repository.snapshot.lock().clone();
    assert_eq!(committed.rules[0].lifecycle().hit_count, 1);
    assert!(committed.rules[0].enabled());
}

async fn assert_production_encode_failure_rolls_back(
    listener: &ProxyListener,
    workspace: &ProxyWorkspace,
    runtime_rules: Vec<intercept_proxy_domain::RuleDefinition>,
    identity: HttpConnectionIdentity,
) {
    let context = phase10_http_context();
    let failing_rpc = Arc::new(RecordingHttpRpc::failing(ExternalPackageCallStage::Encode));
    let failing_snapshot =
        prepared_external_snapshot_for(failing_rpc.clone(), workspace, listener).await;
    let failing_repository = Arc::new(Phase10ActorRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
            Some(Uuid::from_u128(905)),
            7,
            runtime_rules,
            workspace.http_runtime_rule_execution_order(),
        )),
        commit_attempts: AtomicUsize::new(0),
    });
    let failing_pipeline = actor_pipeline(&failing_snapshot, Arc::clone(&failing_repository));
    let failing_identity = HttpConnectionIdentity {
        connection_id: Uuid::from_u128(906),
        ..identity
    };
    let failing_connection = actor_context(&failing_snapshot, &failing_identity);
    failing_pipeline
        .runtime_started(failing_connection.runtime_epoch)
        .await;
    failing_pipeline
        .connection_opened(&failing_connection)
        .await;
    let mut failing_capabilities = failing_snapshot.create_upstream(failing_identity).unwrap();
    let document = failing_capabilities.decode.decode(&context).await.unwrap();
    failing_capabilities.rules.apply(document).await.unwrap();
    let mut failing_message = Message::from_raw_http1_head(
        context.header.as_bytes(),
        Bytes::copy_from_slice(&context.wire_body),
    )
    .unwrap();
    let original_message = failing_message.clone();
    let before = failing_repository.snapshot.lock().clone();
    let error = failing_pipeline
        .apply_request_policy(
            &failing_connection,
            &http_request_metadata(),
            &mut failing_message,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "EXTERNAL_PACKAGE_CALL_FAILED");
    assert_ne!(
        error.code,
        intercept_proxy_runtime::ErrorCode::Internal.as_str()
    );
    assert_eq!(
        error
            .external_package_call
            .as_ref()
            .and_then(|failure| failure.stable_code.as_deref()),
        Some("BODY_ENCODE_FAILED")
    );
    assert_eq!(failing_message.body, original_message.body);
    assert_eq!(failing_message.headers, original_message.headers);
    assert_eq!(failing_rpc.encode_calls.load(Ordering::SeqCst), 1);
    assert_eq!(failing_repository.commit_attempts.load(Ordering::SeqCst), 0);
    assert_eq!(*failing_repository.snapshot.lock(), before);
}
