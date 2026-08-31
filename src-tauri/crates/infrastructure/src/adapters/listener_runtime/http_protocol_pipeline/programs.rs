use std::sync::Arc;

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    DocumentSchemaNode, HttpRuleContent, ProtocolPackageRef, ProtocolRuleStage, ProxyListener,
    ProxyWorkspace, RuleContent, RuleProgramEntry, RuleStage, UnifiedRuleProgram,
    validate_unified_actions_schema,
};

#[derive(Clone)]
pub(super) struct HttpDocumentRulePrograms {
    proxy_to_upstream: Arc<UnifiedRuleProgram>,
    proxy_to_app: Arc<UnifiedRuleProgram>,
}

impl HttpDocumentRulePrograms {
    pub(super) fn program(&self, stage: ProtocolRuleStage) -> Arc<UnifiedRuleProgram> {
        Arc::clone(match stage {
            ProtocolRuleStage::ProxyToUpstream => &self.proxy_to_upstream,
            ProtocolRuleStage::ProxyToApp => &self.proxy_to_app,
        })
    }
}

pub(super) fn compile_programs(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &ProtocolPackageRef,
    upstream_schema: &DocumentSchemaNode,
    downstream_schema: &DocumentSchemaNode,
) -> AppResult<HttpDocumentRulePrograms> {
    let compile = |stage| {
        compile_program(
            workspace,
            listener,
            package,
            schema_for_stage(stage, upstream_schema, downstream_schema),
            stage,
        )
        .map(Arc::new)
    };
    Ok(HttpDocumentRulePrograms {
        proxy_to_upstream: compile(ProtocolRuleStage::ProxyToUpstream)?,
        proxy_to_app: compile(ProtocolRuleStage::ProxyToApp)?,
    })
}

const fn schema_for_stage<'a>(
    stage: ProtocolRuleStage,
    upstream: &'a DocumentSchemaNode,
    downstream: &'a DocumentSchemaNode,
) -> &'a DocumentSchemaNode {
    match stage {
        ProtocolRuleStage::ProxyToUpstream => upstream,
        ProtocolRuleStage::ProxyToApp => downstream,
    }
}

fn compile_program(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &ProtocolPackageRef,
    schema: &DocumentSchemaNode,
    stage: ProtocolRuleStage,
) -> AppResult<UnifiedRuleProgram> {
    let expected_stage = match stage {
        ProtocolRuleStage::ProxyToUpstream => RuleStage::ProxyToUpstream,
        ProtocolRuleStage::ProxyToApp => RuleStage::ProxyToApp,
    };
    let mut entries = Vec::new();
    for definition in &workspace.rule_definitions {
        let RuleContent::Http(HttpRuleContent {
            condition,
            actions,
            document: Some(document),
            ..
        }) = definition.content()
        else {
            continue;
        };
        if definition.listener_id() != listener.id || definition.stage() != expected_stage {
            continue;
        }
        if &document.package != package {
            return Err(AppError::new(
                "DOCUMENT_RULE_RUNTIME_BINDING_MISMATCH",
                "报文规则与当前协议包或 Schema 不一致。",
            )
            .entity(definition.rule_id().to_string()));
        }
        condition.validate_document_schema(schema)?;
        validate_unified_actions_schema(actions, schema)?;
        entries.push(RuleProgramEntry::new(
            definition.rule_id(),
            definition.priority(),
            definition.created_order(),
            condition.clone(),
            actions.clone(),
        )?);
    }
    UnifiedRuleProgram::new(entries).map_err(AppError::from)
}
