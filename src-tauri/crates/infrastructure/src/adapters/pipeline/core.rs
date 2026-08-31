use super::{
    Arc, AtomicU64, BodyCodec, BreakpointCoordinator, CaptureRepositoryAdapter, ConnectionContext,
    DomainMessageStage, EvaluatedRules, EventHub, HttpRequestMetadata, InMemorySessionStore,
    Message, Mutex, PipelineState, ProxyResult, RuleRuntimeService, RuntimeBodyCodecResolver,
    RuntimePipelineAdapter, RuntimePipelineProductHooks, RuntimeRuleRepository,
};

impl RuntimePipelineAdapter {
    pub fn new(
        product: RuntimePipelineProductHooks,
        rules: Arc<dyn RuntimeRuleRepository>,
        sessions: Arc<InMemorySessionStore>,
        breakpoints: Arc<BreakpointCoordinator>,
        events: Arc<EventHub>,
        captures: Arc<CaptureRepositoryAdapter>,
    ) -> Self {
        let rule_runtime =
            RuleRuntimeService::new(product.channel_labels.clone(), rules, Arc::clone(&events));
        Self {
            body_codec: product.body_codec,
            body_codec_resolver: None,
            request_classifier: product.request_classifier,
            channel_labels: product.channel_labels,
            sessions,
            breakpoints,
            events,
            captures,
            capture_cursor: AtomicU64::new(0),
            rule_runtime,
            joint_http_rules: Arc::new(super::JointHttpRuleRuntime::default()),
            state: Mutex::new(PipelineState::default()),
        }
    }

    #[must_use]
    pub fn with_body_codec_resolver(mut self, resolver: Arc<dyn RuntimeBodyCodecResolver>) -> Self {
        self.body_codec_resolver = Some(resolver);
        self
    }

    #[must_use]
    pub(crate) fn with_joint_http_rules(
        mut self,
        runtime: Arc<super::JointHttpRuleRuntime>,
    ) -> Self {
        self.joint_http_rules = runtime;
        self
    }

    pub(super) fn codec_for(
        &self,
        context: &ConnectionContext,
        stage: DomainMessageStage,
        message: &Message,
    ) -> ProxyResult<Arc<dyn BodyCodec>> {
        let resolved = self
            .body_codec_resolver
            .as_ref()
            .map(|resolver| resolver.resolve(context, stage, message))
            .transpose()?
            .flatten();
        Ok(resolved.unwrap_or_else(|| Arc::clone(&self.body_codec)))
    }

    pub(super) async fn evaluate(
        &self,
        context: &ConnectionContext,
        stage: DomainMessageStage,
        request: &HttpRequestMetadata,
        message: Option<&Message>,
        body_codec: Arc<dyn BodyCodec>,
    ) -> ProxyResult<EvaluatedRules> {
        let joint_document = self
            .joint_http_rules
            .take(context, matches!(stage, DomainMessageStage::Response));
        self.rule_runtime
            .evaluate(context, stage, request, message, body_codec, joint_document)
            .await
    }

    pub(super) fn channel_label(&self, channel_id: &str) -> String {
        self.channel_labels
            .get(channel_id)
            .cloned()
            .unwrap_or_else(|| channel_id.to_owned())
    }
}
