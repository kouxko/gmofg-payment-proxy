use std::sync::atomic::{AtomicUsize, Ordering};

use intercept_proxy_domain::{ChannelId, MessageStage, Revision, Rule, RuleId};
use intercept_proxy_runtime::{SocketJointEvaluation, SocketPayloadDirection};

use super::*;

#[derive(Debug)]
struct RecordingSocketPipeline {
    rule: Option<Rule>,
    calls: AtomicUsize,
}

impl intercept_proxy_runtime::HandshakePolicy for RecordingSocketPipeline {}

#[async_trait]
impl PipelinePorts for RecordingSocketPipeline {
    async fn apply_socket_policy(
        &self,
        _context: &intercept_proxy_runtime::ConnectionContext,
        _direction: SocketPayloadDirection,
        mut evaluation: Box<dyn SocketJointEvaluation>,
    ) -> intercept_proxy_runtime::Result<SocketContext> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(rule) = &self.rule {
            evaluation.gate(rule.id.as_uuid(), 1)?;
        }
        evaluation.encode().await.map_err(|error| {
            intercept_proxy_runtime::ProxyError::new(
                intercept_proxy_runtime::ErrorCode::ExternalPackageCallFailed,
                error.message,
            )
            .with_external_package_call(error.external_package_call)
        })
    }
}

fn factory(
    rule: Option<Rule>,
    fail_encode: bool,
) -> (ExternalSocketCapabilityFactoryAdapter, Arc<FakeExternalRpc>) {
    factory_with_program(rule, fail_encode, None)
}

fn factory_with_program(
    rule: Option<Rule>,
    fail_encode: bool,
    program: Option<Arc<intercept_proxy_domain::UnifiedRuleProgram>>,
) -> (ExternalSocketCapabilityFactoryAdapter, Arc<FakeExternalRpc>) {
    let registration = registration();
    let rpc = Arc::new(FakeExternalRpc {
        fail_encode,
        ..FakeExternalRpc::default()
    });
    let document_rules = program.map_or_else(
        || rules(&registration),
        |upstream| {
            ProtocolDocumentRuleConnectionFactory::new_unified(
                listener_id(),
                registration.package().identity().clone(),
                upstream,
                Arc::new(intercept_proxy_domain::UnifiedRuleProgram::new(Vec::new()).unwrap()),
            )
        },
    );
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), rpc.clone()),
        document_rules,
        SocketTopology::default(),
    );
    let pipeline = Arc::new(RecordingSocketPipeline {
        rule,
        calls: AtomicUsize::new(0),
    });
    (
        ExternalSocketCapabilityFactoryAdapter::new_with_pipeline(
            &snapshot,
            observation_metadata(),
            pipeline,
        ),
        rpc,
    )
}

#[tokio::test]
async fn production_joint_pipeline_executes_recursive_or_insert_and_append() {
    use intercept_proxy_domain::{
        Condition, ConditionTree, DocumentMutation, DocumentPredicate, RuleProgramEntry,
        StringOperator, StringPredicate, UnifiedAction, UnifiedRuleProgram,
    };

    let leaf = |expected: &str| {
        ConditionTree::Leaf(Condition::Document {
            path: JsonPointer::property("message_type"),
            predicate: DocumentPredicate::String(StringPredicate {
                operator: StringOperator::Equal,
                value: expected.to_owned(),
            }),
        })
    };
    let program = UnifiedRuleProgram::new(vec![
        RuleProgramEntry::new(
            RuleId::from_uuid(Uuid::from_u128(44)),
            10,
            1,
            ConditionTree::Any(vec![leaf("0200"), leaf("fallback")]),
            vec![
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
        )
        .unwrap(),
    ])
    .unwrap();
    let (factory, rpc) = factory_with_program(Some(runtime_rule()), false, Some(Arc::new(program)));
    let mut capabilities = factory.create_upstream(connection()).unwrap();
    let original = SocketContext {
        data: b"abc".to_vec(),
    };
    let document = capabilities.decode.decode(&original).await.unwrap();
    capabilities.rules.apply(document).await.unwrap();

    assert_eq!(
        serde_json::to_value(rpc.encoded_document.lock().clone().unwrap()).unwrap()["items"],
        json!(["first", "last"])
    );
}

#[tokio::test]
async fn production_joint_pipeline_preserves_unchanged_bytes_without_encode_rpc() {
    let (factory, rpc) = factory(None, false);
    let mut capabilities = factory.create_upstream(connection()).unwrap();
    let original = SocketContext {
        data: b"abc".to_vec(),
    };
    let document = capabilities.decode.decode(&original).await.unwrap();
    let document = capabilities.rules.apply(document).await.unwrap();
    let encoded = capabilities
        .encode
        .encode(&original, &document)
        .await
        .unwrap();
    assert_eq!(encoded.data, original.data);
    assert!(!rpc.calls.lock().contains(&"hooks.upstream.encode"));
}

#[tokio::test]
async fn production_joint_pipeline_changes_document_before_encode_rpc() {
    let (factory, rpc) = factory(Some(runtime_rule()), false);
    let mut capabilities = factory.create_upstream(connection()).unwrap();
    let original = SocketContext {
        data: b"abc".to_vec(),
    };
    let document = capabilities.decode.decode(&original).await.unwrap();
    let document = capabilities.rules.apply(document).await.unwrap();
    let encoded = capabilities
        .encode
        .encode(&original, &document)
        .await
        .unwrap();
    assert_eq!(encoded.data, b"encoded");
    assert_eq!(
        serde_json::to_value(rpc.encoded_document.lock().clone().unwrap()).unwrap()["amount"],
        json!(42.0)
    );
}

#[tokio::test]
async fn production_joint_pipeline_encode_failure_preserves_typed_identity() {
    let (factory, _) = factory(Some(runtime_rule()), true);
    let mut capabilities = factory.create_upstream(connection()).unwrap();
    let original = SocketContext {
        data: b"abc".to_vec(),
    };
    let document = capabilities.decode.decode(&original).await.unwrap();
    let error = capabilities.rules.apply(document).await.unwrap_err();
    let failure = error.external_package_call.expect("typed Encode failure");
    assert_eq!(
        failure.stage,
        intercept_proxy_exchange::ExternalPackageCallStage::Encode
    );
    assert_eq!(failure.request_id.as_deref(), Some("phase11-encode-1"));
    assert_eq!(failure.stable_code.as_deref(), Some("BODY_ENCODE_FAILED"));
}

fn runtime_rule() -> Rule {
    Rule {
        id: RuleId::from_uuid(Uuid::from_u128(44)),
        revision: Revision::INITIAL,
        name: "socket document gate".into(),
        description: String::new(),
        enabled: true,
        priority: 10,
        created_order: 1,
        channel: Some(ChannelId::new(listener_id().to_string()).unwrap()),
        stage: MessageStage::Request,
        conditions: Vec::new(),
        actions: Vec::new(),
        one_shot: false,
        hit_count: 0,
        last_hit_at: None,
    }
}
