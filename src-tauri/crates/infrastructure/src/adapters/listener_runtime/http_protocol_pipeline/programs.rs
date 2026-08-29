use std::sync::Arc;

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    DocumentSchemaNode, ProtocolDocumentRuleDefinition, ProtocolDocumentRuleProgram,
    ProtocolRuleStage, ProxyListener, ProxyWorkspace, sort_protocol_document_rules,
};

#[derive(Clone)]
pub(super) struct HttpDocumentRulePrograms {
    app_to_proxy: Arc<ProtocolDocumentRuleProgram>,
    proxy_to_upstream: Arc<ProtocolDocumentRuleProgram>,
    upstream_to_proxy: Arc<ProtocolDocumentRuleProgram>,
    proxy_to_app: Arc<ProtocolDocumentRuleProgram>,
}

impl HttpDocumentRulePrograms {
    pub(super) fn program(&self, stage: ProtocolRuleStage) -> Arc<ProtocolDocumentRuleProgram> {
        Arc::clone(match stage {
            ProtocolRuleStage::AppToProxy => &self.app_to_proxy,
            ProtocolRuleStage::ProxyToUpstream => &self.proxy_to_upstream,
            ProtocolRuleStage::UpstreamToProxy => &self.upstream_to_proxy,
            ProtocolRuleStage::ProxyToApp => &self.proxy_to_app,
        })
    }
}

pub(super) fn compile_programs(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &intercept_proxy_domain::ProtocolPackageRef,
    upstream_schema: &DocumentSchemaNode,
    downstream_schema: &DocumentSchemaNode,
) -> AppResult<HttpDocumentRulePrograms> {
    let mut rules = workspace
        .document_runtime_rules()?
        .into_iter()
        .filter(|rule| rule.listener_id() == listener.id)
        .collect::<Vec<_>>();
    for rule in &rules {
        let schema = schema_for_stage(rule.stage(), upstream_schema, downstream_schema);
        if rule.package() != package {
            return Err(AppError::new(
                "DOCUMENT_RULE_RUNTIME_BINDING_MISMATCH",
                "报文规则与当前协议包或 Schema 不一致。",
            )
            .entity(rule.rule_id().to_string()));
        }
        rule.validate_against_schema(schema)?;
    }
    sort_protocol_document_rules(&mut rules);
    let program = |stage| {
        compile_program(
            listener,
            package,
            schema_for_stage(stage, upstream_schema, downstream_schema),
            stage,
            &rules,
        )
        .map(Arc::new)
    };
    Ok(HttpDocumentRulePrograms {
        app_to_proxy: program(ProtocolRuleStage::AppToProxy)?,
        proxy_to_upstream: program(ProtocolRuleStage::ProxyToUpstream)?,
        upstream_to_proxy: program(ProtocolRuleStage::UpstreamToProxy)?,
        proxy_to_app: program(ProtocolRuleStage::ProxyToApp)?,
    })
}

const fn schema_for_stage<'a>(
    stage: ProtocolRuleStage,
    upstream: &'a DocumentSchemaNode,
    downstream: &'a DocumentSchemaNode,
) -> &'a DocumentSchemaNode {
    match stage {
        ProtocolRuleStage::AppToProxy | ProtocolRuleStage::ProxyToUpstream => upstream,
        ProtocolRuleStage::UpstreamToProxy | ProtocolRuleStage::ProxyToApp => downstream,
    }
}

fn compile_program(
    listener: &ProxyListener,
    package: &intercept_proxy_domain::ProtocolPackageRef,
    schema: &intercept_proxy_domain::DocumentSchemaNode,
    stage: ProtocolRuleStage,
    rules: &[ProtocolDocumentRuleDefinition],
) -> AppResult<ProtocolDocumentRuleProgram> {
    ProtocolDocumentRuleProgram::new_for_stage(
        listener.id,
        package.clone(),
        schema.clone(),
        stage,
        rules
            .iter()
            .filter(|rule| rule.stage() == stage)
            .cloned()
            .collect(),
    )
    .map_err(AppError::from)
}
