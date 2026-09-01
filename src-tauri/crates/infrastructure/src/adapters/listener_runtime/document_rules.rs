//! Immutable two-stage Socket Document programs shared by the external package pipeline.

use std::sync::Arc;

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    DocumentSchemaNode, ListenerId, ProtocolDirection, ProtocolPackageRef, ProxyListener,
    ProxyWorkspace, RuleContent, RuleProgramEntry, RuleStage, SocketRuleContent, SocketTopology,
    UnifiedRuleProgram, validate_unified_action_schema,
};
use parking_lot::RwLock;

#[derive(Clone)]
pub struct DocumentProgramFactory {
    programs: Arc<RwLock<DocumentPrograms>>,
}

#[derive(Clone)]
struct DocumentPrograms {
    listener_id: ListenerId,
    package: ProtocolPackageRef,
    proxy_to_upstream: Arc<UnifiedRuleProgram>,
    proxy_to_app: Arc<UnifiedRuleProgram>,
}

impl std::fmt::Debug for DocumentProgramFactory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let programs = self.programs.read();
        formatter
            .debug_struct("DocumentProgramFactory")
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

impl DocumentProgramFactory {
    pub(crate) fn new(
        listener_id: ListenerId,
        package: ProtocolPackageRef,
        proxy_to_upstream: Arc<UnifiedRuleProgram>,
        proxy_to_app: Arc<UnifiedRuleProgram>,
    ) -> Self {
        Self {
            programs: Arc::new(RwLock::new(DocumentPrograms {
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

    pub(crate) fn replace(&self, replacement: &Self) {
        *self.programs.write() = replacement.programs.read().clone();
    }
}

pub(super) fn compile_document_rules(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &ProtocolPackageRef,
    upstream_schema: Option<&DocumentSchemaNode>,
    downstream_schema: Option<&DocumentSchemaNode>,
    topology: &SocketTopology,
) -> AppResult<DocumentProgramFactory> {
    let compile = |stage: RuleStage| {
        validate_rule_direction(stage, topology)?;
        let schema = match stage {
            RuleStage::ProxyToUpstream => upstream_schema,
            RuleStage::ProxyToApp => downstream_schema,
        };
        let mut entries = Vec::new();
        for definition in &workspace.rule_definitions {
            let RuleContent::Socket(SocketRuleContent {
                package: rule_package,
                condition,
                action,
            }) = definition.content()
            else {
                continue;
            };
            if definition.listener_id() != listener.id || definition.stage() != stage {
                continue;
            }
            if rule_package != package {
                return Err(AppError::new(
                    "PROTOCOL_RULE_RUNTIME_BINDING_MISMATCH",
                    "协议报文规则与当前协议包或 Schema 不一致。",
                )
                .entity(definition.rule_id().to_string()));
            }
            if let Some(schema) = schema {
                intercept_proxy_domain::validate_document_condition_schema(condition, schema)?;
                validate_unified_action_schema(action, schema)?;
            }
            entries.push(RuleProgramEntry::new(
                definition.rule_id(),
                definition.priority(),
                definition.created_order(),
                condition.clone(),
                action.clone(),
            )?);
        }
        UnifiedRuleProgram::new(entries)
            .map(Arc::new)
            .map_err(AppError::from)
    };
    Ok(DocumentProgramFactory::new(
        listener.id,
        package.clone(),
        compile(RuleStage::ProxyToUpstream)?,
        compile(RuleStage::ProxyToApp)?,
    ))
}

fn validate_rule_direction(stage: RuleStage, topology: &SocketTopology) -> AppResult<()> {
    match topology {
        SocketTopology::LocalResponder(_)
            if !matches!(stage, RuleStage::ProxyToUpstream | RuleStage::ProxyToApp) =>
        {
            Err(AppError::new(
                "PROTOCOL_RULE_DIRECTION_INVALID",
                "本机应答运行快照只接受两个统一代理写出阶段。",
            ))
        }
        SocketTopology::Relay(_) | SocketTopology::LocalResponder(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use intercept_proxy_domain::{
        Condition, DocumentMutation, DocumentPredicate, DocumentValue, JsonPointer,
        ProtocolPackageId, ProtocolPackageVersion, RuleDefinition, RuleDefinitionDraft,
        StringOperator, StringPredicate, UnifiedAction,
    };

    use super::*;

    #[test]
    fn absent_socket_schema_keeps_root_condition_and_root_set_capability() {
        let listener = ProxyListener::default();
        let package = ProtocolPackageRef {
            id: ProtocolPackageId::new("schema-free-socket").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        };
        let definition = RuleDefinition::create(
            RuleDefinitionDraft {
                name: "schema-free socket root".into(),
                enabled: true,
                priority: 1,
                listener_id: listener.id,
                stage: RuleStage::ProxyToUpstream,
                content: RuleContent::Socket(SocketRuleContent {
                    package: package.clone(),
                    condition: Condition::Document {
                        path: JsonPointer::root(),
                        predicate: DocumentPredicate::String(StringPredicate {
                            operator: StringOperator::Equal,
                            value: "before".into(),
                        }),
                    },
                    action: UnifiedAction::Document(DocumentMutation::Set {
                        path: JsonPointer::root(),
                        value: DocumentValue::String("after".into()),
                    }),
                }),
            },
            1,
        )
        .unwrap();
        let workspace = ProxyWorkspace {
            rule_definitions: vec![definition],
            ..ProxyWorkspace::default()
        };

        let programs = compile_document_rules(
            &workspace,
            &listener,
            &package,
            None,
            None,
            &SocketTopology::default(),
        )
        .unwrap();

        assert_eq!(
            programs.direction_programs(ProtocolDirection::Upstream)[0]
                .rules()
                .len(),
            1
        );
    }
}
