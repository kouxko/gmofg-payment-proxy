use super::{
    Arc, AtomicU64, BodyCodec, BreakpointCoordinator, CaptureRepositoryAdapter, ConnectionContext,
    DomainMessageStage, EvaluatedRules, EventHub, InMemorySessionStore, Message, Mutex,
    PipelineState, ProxyResult, RuleRuntimeService, RuntimeBodyCodecResolver,
    RuntimePipelineAdapter, RuntimePipelineProductHooks, RuntimeRuleRepository,
    RuntimeWorkspacePolicyEvaluation, RuntimeWorkspacePolicyResolver,
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
            workspace_policy_resolver: None,
            request_classifier: product.request_classifier,
            channel_labels: product.channel_labels,
            sessions,
            breakpoints,
            events,
            captures,
            capture_cursor: AtomicU64::new(0),
            rule_runtime,
            state: Mutex::new(PipelineState::default()),
        }
    }

    #[must_use]
    pub fn with_body_codec_resolver(mut self, resolver: Arc<dyn RuntimeBodyCodecResolver>) -> Self {
        self.body_codec_resolver = Some(resolver);
        self
    }

    #[must_use]
    pub fn with_workspace_policy_resolver(
        mut self,
        resolver: Arc<dyn RuntimeWorkspacePolicyResolver>,
    ) -> Self {
        self.workspace_policy_resolver = Some(resolver);
        self
    }

    pub(super) fn evaluate_workspace_policies(
        &self,
        context: &ConnectionContext,
        stage: DomainMessageStage,
        message: &Message,
        body_codec: &dyn BodyCodec,
    ) -> ProxyResult<RuntimeWorkspacePolicyEvaluation> {
        self.workspace_policy_resolver.as_ref().map_or_else(
            || Ok(RuntimeWorkspacePolicyEvaluation::default()),
            |resolver| resolver.evaluate(context, stage, message, body_codec),
        )
    }

    pub(super) fn codec_for(
        &self,
        context: &ConnectionContext,
        stage: DomainMessageStage,
    ) -> ProxyResult<Arc<dyn BodyCodec>> {
        let resolved = self
            .body_codec_resolver
            .as_ref()
            .map(|resolver| resolver.resolve(context, stage))
            .transpose()?
            .flatten();
        Ok(resolved.unwrap_or_else(|| Arc::clone(&self.body_codec)))
    }

    pub(super) fn evaluate(
        &self,
        context: &ConnectionContext,
        stage: DomainMessageStage,
        message: Option<&Message>,
        body_codec: &dyn BodyCodec,
    ) -> ProxyResult<EvaluatedRules> {
        self.rule_runtime
            .evaluate(context, stage, message, body_codec)
    }

    pub(super) fn channel_label(&self, channel_id: &str) -> String {
        self.channel_labels
            .get(channel_id)
            .cloned()
            .unwrap_or_else(|| channel_id.to_owned())
    }
}
