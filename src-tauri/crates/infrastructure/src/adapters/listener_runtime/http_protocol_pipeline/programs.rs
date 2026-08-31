use std::sync::Arc;

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    DocumentSchemaNode, HttpRuleContent, ProtocolDirection, ProtocolPackageRef, ProxyListener,
    ProxyWorkspace, RuleContent, RuleProgramEntry, RuleStage, UnifiedRuleProgram,
    validate_unified_actions_schema,
};

#[derive(Clone)]
pub(crate) struct HttpDocumentRulePrograms {
    proxy_to_upstream: Arc<UnifiedRuleProgram>,
    proxy_to_app: Arc<UnifiedRuleProgram>,
}

impl HttpDocumentRulePrograms {
    pub(super) fn program(&self, direction: ProtocolDirection) -> Arc<UnifiedRuleProgram> {
        Arc::clone(match direction {
            ProtocolDirection::Upstream => &self.proxy_to_upstream,
            ProtocolDirection::Downstream => &self.proxy_to_app,
        })
    }
}

pub(super) fn compile_programs(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &ProtocolPackageRef,
    upstream_schema: Option<&DocumentSchemaNode>,
    downstream_schema: Option<&DocumentSchemaNode>,
) -> AppResult<HttpDocumentRulePrograms> {
    Ok(HttpDocumentRulePrograms {
        proxy_to_upstream: Arc::new(compile_program(
            workspace,
            listener,
            package,
            upstream_schema,
            RuleStage::ProxyToUpstream,
        )?),
        proxy_to_app: Arc::new(compile_program(
            workspace,
            listener,
            package,
            downstream_schema,
            RuleStage::ProxyToApp,
        )?),
    })
}

fn compile_program(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    package: &ProtocolPackageRef,
    schema: Option<&DocumentSchemaNode>,
    stage: RuleStage,
) -> AppResult<UnifiedRuleProgram> {
    let mut entries = Vec::new();
    for definition in &workspace.rule_definitions {
        let RuleContent::Http(HttpRuleContent {
            condition,
            actions,
            document,
            ..
        }) = definition.content()
        else {
            continue;
        };
        if definition.listener_id() != listener.id || definition.stage() != stage {
            continue;
        }
        if document
            .as_ref()
            .is_some_and(|document| &document.package != package)
        {
            return Err(AppError::new(
                "DOCUMENT_RULE_RUNTIME_BINDING_MISMATCH",
                "报文规则与当前协议包或 Schema 不一致。",
            )
            .entity(definition.rule_id().to_string()));
        }
        if let Some(schema) = schema {
            condition.validate_document_schema(schema)?;
            validate_unified_actions_schema(actions, schema)?;
        }
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

#[cfg(test)]
mod tests {
    use intercept_proxy_domain::{
        Condition, ConditionTree, DocumentMutation, DocumentPredicate, DocumentValue,
        HttpDocumentRuleContent, HttpRuleContent, JsonPointer, ListenerId, ProtocolPackageId,
        ProtocolPackageVersion, RuleDefinition, RuleDefinitionDraft, StringOperator,
        StringPredicate, UnifiedAction,
    };

    use super::*;

    #[test]
    fn absent_http_schema_keeps_root_condition_and_root_set_capability() {
        let listener = ProxyListener {
            id: ListenerId::new(),
            ..ProxyListener::default()
        };
        let package = ProtocolPackageRef {
            id: ProtocolPackageId::new("schema-free-http").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        };
        let definition = RuleDefinition::create(
            RuleDefinitionDraft {
                name: "schema-free root".into(),
                enabled: true,
                priority: 1,
                listener_id: listener.id,
                stage: RuleStage::ProxyToUpstream,
                one_shot: false,
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    condition: ConditionTree::Leaf(Condition::Document {
                        path: JsonPointer::root(),
                        predicate: DocumentPredicate::String(StringPredicate {
                            operator: StringOperator::Equal,
                            value: "before".into(),
                        }),
                    }),
                    actions: vec![UnifiedAction::Document(DocumentMutation::Set {
                        path: JsonPointer::root(),
                        value: DocumentValue::String("after".into()),
                    })],
                    document: Some(HttpDocumentRuleContent {
                        package: package.clone(),
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

        let program = compile_program(
            &workspace,
            &listener,
            &package,
            None,
            RuleStage::ProxyToUpstream,
        )
        .unwrap();

        assert_eq!(program.rules().len(), 1);
    }
}
