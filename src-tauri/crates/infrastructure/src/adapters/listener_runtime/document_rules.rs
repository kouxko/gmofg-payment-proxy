//! Immutable two-stage Socket Document programs shared by the external package pipeline.

use std::sync::Arc;

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    DocumentSchemaNode, DomainError, ErrorCode, ProtocolDirection, ProtocolDocumentRuleDefinition,
    ProtocolDocumentRuleProgram, ProtocolPackageRef, ProtocolRuleStage, ProxyListener,
    ProxyWorkspace, SocketTopology, sort_protocol_document_rules,
};
use parking_lot::RwLock;

#[derive(Clone)]
pub struct ProtocolDocumentRuleConnectionFactory {
    programs: Arc<RwLock<ProtocolDocumentRulePrograms>>,
}

#[derive(Clone)]
struct ProtocolDocumentRulePrograms {
    proxy_to_upstream: Arc<ProtocolDocumentRuleProgram>,
    proxy_to_app: Arc<ProtocolDocumentRuleProgram>,
}

impl std::fmt::Debug for ProtocolDocumentRuleConnectionFactory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let programs = self.programs.read();
        formatter
            .debug_struct("ProtocolDocumentRuleConnectionFactory")
            .field("listener_id", &programs.proxy_to_upstream.listener_id())
            .field("package", programs.proxy_to_upstream.package())
            .field(
                "proxy_to_upstream",
                &programs.proxy_to_upstream.rules().len(),
            )
            .field("proxy_to_app", &programs.proxy_to_app.rules().len())
            .finish_non_exhaustive()
    }
}

impl ProtocolDocumentRuleConnectionFactory {
    pub(crate) fn new(
        proxy_to_upstream: Arc<ProtocolDocumentRuleProgram>,
        proxy_to_app: Arc<ProtocolDocumentRuleProgram>,
    ) -> Result<Self, DomainError> {
        let programs = [&proxy_to_upstream, &proxy_to_app];
        let expected = [
            ProtocolRuleStage::ProxyToUpstream,
            ProtocolRuleStage::ProxyToApp,
        ];
        for (program, stage) in programs.iter().zip(expected) {
            if program.stage() != stage {
                return Err(binding_error(
                    "factory.stage",
                    "规则 Program 处理阶段不正确",
                ));
            }
            if program.listener_id() != proxy_to_upstream.listener_id()
                || program.package() != proxy_to_upstream.package()
            {
                return Err(binding_error(
                    "factory.binding",
                    "两个处理阶段必须绑定同一入口和协议包",
                ));
            }
        }
        Ok(Self {
            programs: Arc::new(RwLock::new(ProtocolDocumentRulePrograms {
                proxy_to_upstream,
                proxy_to_app,
            })),
        })
    }

    pub(crate) fn direction_programs(
        &self,
        direction: ProtocolDirection,
    ) -> [Arc<ProtocolDocumentRuleProgram>; 1] {
        let programs = self.programs.read();
        match direction {
            ProtocolDirection::Upstream => [Arc::clone(&programs.proxy_to_upstream)],
            ProtocolDirection::Downstream => [Arc::clone(&programs.proxy_to_app)],
        }
    }

    #[cfg(test)]
    pub(crate) fn program(&self, stage: ProtocolRuleStage) -> Arc<ProtocolDocumentRuleProgram> {
        let programs = self.programs.read();
        match stage {
            ProtocolRuleStage::ProxyToUpstream => Arc::clone(&programs.proxy_to_upstream),
            ProtocolRuleStage::ProxyToApp => Arc::clone(&programs.proxy_to_app),
        }
    }

    pub(crate) fn replace(&self, replacement: &Self) {
        *self.programs.write() = replacement.programs.read().clone();
    }
}

fn binding_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "协议 Document 运行时绑定不一致")
        .with_field_error(field, message)
}

fn compile_rule_program(
    listener: &ProxyListener,
    package: &ProtocolPackageRef,
    schema: &DocumentSchemaNode,
    stage: ProtocolRuleStage,
    rules: &[ProtocolDocumentRuleDefinition],
) -> AppResult<ProtocolDocumentRuleProgram> {
    let selected = rules
        .iter()
        .filter(|rule| rule.stage() == stage)
        .cloned()
        .collect();
    ProtocolDocumentRuleProgram::new_for_stage(
        listener.id,
        package.clone(),
        schema.clone(),
        stage,
        selected,
    )
    .map_err(AppError::from)
}

pub(super) fn compile_document_rules(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &ProtocolPackageRef,
    upstream_schema: &DocumentSchemaNode,
    downstream_schema: &DocumentSchemaNode,
    topology: &SocketTopology,
) -> AppResult<ProtocolDocumentRuleConnectionFactory> {
    let mut rules = workspace
        .document_runtime_rules()?
        .into_iter()
        .filter(|rule| rule.listener_id() == listener.id)
        .collect::<Vec<_>>();
    for rule in &rules {
        let schema = match rule.direction() {
            ProtocolDirection::Upstream => upstream_schema,
            ProtocolDirection::Downstream => downstream_schema,
        };
        if rule.package() != package {
            return Err(AppError::new(
                "PROTOCOL_RULE_RUNTIME_BINDING_MISMATCH",
                "协议报文规则与当前协议包或 Schema 不一致。",
            )
            .entity(rule.rule_id().to_string()));
        }
        rule.validate_against_schema(schema)?;
        validate_rule_direction(rule, topology)?;
    }
    sort_protocol_document_rules(&mut rules);
    let compile = |stage: ProtocolRuleStage| {
        let schema = match stage.direction() {
            ProtocolDirection::Upstream => upstream_schema,
            ProtocolDirection::Downstream => downstream_schema,
        };
        compile_rule_program(listener, package, schema, stage, &rules).map(Arc::new)
    };
    ProtocolDocumentRuleConnectionFactory::new(
        compile(ProtocolRuleStage::ProxyToUpstream)?,
        compile(ProtocolRuleStage::ProxyToApp)?,
    )
    .map_err(AppError::from)
}

fn validate_rule_direction(
    rule: &ProtocolDocumentRuleDefinition,
    topology: &SocketTopology,
) -> AppResult<()> {
    match topology {
        SocketTopology::LocalResponder(_)
            if !matches!(
                rule.stage(),
                ProtocolRuleStage::ProxyToUpstream | ProtocolRuleStage::ProxyToApp
            ) =>
        {
            Err(AppError::new(
                "PROTOCOL_RULE_DIRECTION_INVALID",
                "本机应答运行快照只接受两个统一代理写出阶段。",
            )
            .entity(rule.rule_id().to_string()))
        }
        SocketTopology::Relay(_) | SocketTopology::LocalResponder(_) => Ok(()),
    }
}
