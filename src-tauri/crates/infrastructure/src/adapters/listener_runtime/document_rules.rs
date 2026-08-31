//! Immutable two-stage Socket Document programs shared by the external package pipeline.

use std::sync::Arc;

use intercept_proxy_application::{AppError, AppResult};
#[cfg(test)]
use intercept_proxy_domain::{ConditionTree, DomainError, ErrorCode, ProtocolDocumentRuleProgram};
use intercept_proxy_domain::{
    DocumentSchemaNode, ListenerId, ProtocolDirection, ProtocolPackageRef, ProtocolRuleStage,
    ProxyListener, ProxyWorkspace, RuleContent, RuleProgramEntry, RuleStage, SocketRuleContent,
    SocketTopology, UnifiedRuleProgram, validate_unified_actions_schema,
};
use parking_lot::RwLock;

#[derive(Clone)]
pub struct ProtocolDocumentRuleConnectionFactory {
    programs: Arc<RwLock<ProtocolDocumentRulePrograms>>,
}

#[derive(Clone)]
struct ProtocolDocumentRulePrograms {
    listener_id: ListenerId,
    package: ProtocolPackageRef,
    proxy_to_upstream: Arc<UnifiedRuleProgram>,
    proxy_to_app: Arc<UnifiedRuleProgram>,
}

impl std::fmt::Debug for ProtocolDocumentRuleConnectionFactory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let programs = self.programs.read();
        formatter
            .debug_struct("ProtocolDocumentRuleConnectionFactory")
            .field("listener_id", &programs.listener_id)
            .field("package", &programs.package)
            .field(
                "proxy_to_upstream",
                &programs.proxy_to_upstream.rules().len(),
            )
            .field("proxy_to_app", &programs.proxy_to_app.rules().len())
            .finish_non_exhaustive()
    }
}

impl ProtocolDocumentRuleConnectionFactory {
    #[cfg(test)]
    pub(crate) fn new(
        proxy_to_upstream: &ProtocolDocumentRuleProgram,
        proxy_to_app: &ProtocolDocumentRuleProgram,
    ) -> Result<Self, DomainError> {
        let programs = [proxy_to_upstream, proxy_to_app];
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
                listener_id: proxy_to_upstream.listener_id(),
                package: proxy_to_upstream.package().clone(),
                proxy_to_upstream: Arc::new(unify_legacy_program(proxy_to_upstream)?),
                proxy_to_app: Arc::new(unify_legacy_program(proxy_to_app)?),
            })),
        })
    }

    pub(crate) fn new_unified(
        listener_id: ListenerId,
        package: ProtocolPackageRef,
        proxy_to_upstream: Arc<UnifiedRuleProgram>,
        proxy_to_app: Arc<UnifiedRuleProgram>,
    ) -> Self {
        Self {
            programs: Arc::new(RwLock::new(ProtocolDocumentRulePrograms {
                listener_id,
                package,
                proxy_to_upstream,
                proxy_to_app,
            })),
        }
    }

    pub(crate) fn direction_programs(
        &self,
        direction: ProtocolDirection,
    ) -> [Arc<UnifiedRuleProgram>; 1] {
        let programs = self.programs.read();
        match direction {
            ProtocolDirection::Upstream => [Arc::clone(&programs.proxy_to_upstream)],
            ProtocolDirection::Downstream => [Arc::clone(&programs.proxy_to_app)],
        }
    }

    #[cfg(test)]
    pub(crate) fn program(&self, stage: ProtocolRuleStage) -> Arc<UnifiedRuleProgram> {
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

#[cfg(test)]
fn binding_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "协议 Document 运行时绑定不一致")
        .with_field_error(field, message)
}

#[cfg(test)]
fn unify_legacy_program(
    program: &ProtocolDocumentRuleProgram,
) -> Result<UnifiedRuleProgram, DomainError> {
    UnifiedRuleProgram::new(
        program
            .rules()
            .iter()
            .map(|rule| {
                RuleProgramEntry::new(
                    intercept_proxy_domain::RuleId::from_uuid(rule.rule_id().as_uuid()),
                    rule.priority(),
                    rule.created_order(),
                    ConditionTree::from_document_conditions(rule.conditions().iter().cloned()),
                    rule.actions().iter().cloned().map(Into::into).collect(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?,
    )
}

pub(super) fn compile_document_rules(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &ProtocolPackageRef,
    upstream_schema: &DocumentSchemaNode,
    downstream_schema: &DocumentSchemaNode,
    topology: &SocketTopology,
) -> AppResult<ProtocolDocumentRuleConnectionFactory> {
    let compile = |stage: ProtocolRuleStage| {
        validate_rule_direction(stage, topology)?;
        let schema = match stage.direction() {
            ProtocolDirection::Upstream => upstream_schema,
            ProtocolDirection::Downstream => downstream_schema,
        };
        let expected_stage = match stage {
            ProtocolRuleStage::ProxyToUpstream => RuleStage::ProxyToUpstream,
            ProtocolRuleStage::ProxyToApp => RuleStage::ProxyToApp,
        };
        let mut entries = Vec::new();
        for definition in &workspace.rule_definitions {
            let RuleContent::Socket(SocketRuleContent {
                package: rule_package,
                condition,
                actions,
            }) = definition.content()
            else {
                continue;
            };
            if definition.listener_id() != listener.id || definition.stage() != expected_stage {
                continue;
            }
            if rule_package != package {
                return Err(AppError::new(
                    "PROTOCOL_RULE_RUNTIME_BINDING_MISMATCH",
                    "协议报文规则与当前协议包或 Schema 不一致。",
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
        UnifiedRuleProgram::new(entries)
            .map(Arc::new)
            .map_err(AppError::from)
    };
    Ok(ProtocolDocumentRuleConnectionFactory::new_unified(
        listener.id,
        package.clone(),
        compile(ProtocolRuleStage::ProxyToUpstream)?,
        compile(ProtocolRuleStage::ProxyToApp)?,
    ))
}

fn validate_rule_direction(stage: ProtocolRuleStage, topology: &SocketTopology) -> AppResult<()> {
    match topology {
        SocketTopology::LocalResponder(_)
            if !matches!(
                stage,
                ProtocolRuleStage::ProxyToUpstream | ProtocolRuleStage::ProxyToApp
            ) =>
        {
            Err(AppError::new(
                "PROTOCOL_RULE_DIRECTION_INVALID",
                "本机应答运行快照只接受两个统一代理写出阶段。",
            ))
        }
        SocketTopology::Relay(_) | SocketTopology::LocalResponder(_) => Ok(()),
    }
}
