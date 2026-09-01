use std::sync::Arc;

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    DocumentSchemaNode, HttpRuleContent, ProtocolDirection, ProxyListener, ProxyWorkspace,
    RuleContent, RuleProgramEntry, RuleStage, UnifiedRuleProgram, is_document_condition,
    validate_unified_action_schema,
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

    pub(super) fn has_rules(&self, direction: ProtocolDirection) -> bool {
        !self.program(direction).rules().is_empty()
    }
}

pub(super) fn compile_programs(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    upstream_schema: Option<&DocumentSchemaNode>,
    downstream_schema: Option<&DocumentSchemaNode>,
    owns_all_http: bool,
) -> AppResult<HttpDocumentRulePrograms> {
    Ok(HttpDocumentRulePrograms {
        proxy_to_upstream: Arc::new(compile_program(
            workspace,
            listener,
            upstream_schema,
            owns_all_http,
            RuleStage::ProxyToUpstream,
        )?),
        proxy_to_app: Arc::new(compile_program(
            workspace,
            listener,
            downstream_schema,
            owns_all_http,
            RuleStage::ProxyToApp,
        )?),
    })
}

fn compile_program(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    schema: Option<&DocumentSchemaNode>,
    owns_all_http: bool,
    stage: RuleStage,
) -> AppResult<UnifiedRuleProgram> {
    let definitions = workspace
        .rule_definitions
        .iter()
        .filter(|definition| definition.listener_id() == listener.id && definition.stage() == stage)
        .filter_map(|definition| match definition.content() {
            RuleContent::Http(content) => Some((definition, content)),
            RuleContent::Socket(_) => None,
        })
        .collect::<Vec<_>>();
    let owns_direction = owns_all_http
        || definitions.iter().any(|(_, content)| {
            is_document_condition(&content.condition)
                || matches!(
                    content.action,
                    intercept_proxy_domain::UnifiedAction::Document(_)
                )
        });
    if !owns_direction {
        return UnifiedRuleProgram::new(Vec::new()).map_err(AppError::from);
    }
    let mut entries = Vec::new();
    for (definition, content) in definitions {
        let HttpRuleContent {
            condition, action, ..
        } = content;
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
    UnifiedRuleProgram::new(entries).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use intercept_proxy_domain::{
        Condition, DocumentMutation, DocumentPredicate, DocumentValue, HttpRuleContent,
        JsonPointer, ListenerId, RuleDefinition, RuleDefinitionDraft, StringOperator,
        StringPredicate, UnifiedAction,
    };

    use super::*;

    #[test]
    fn absent_http_schema_keeps_root_condition_and_root_set_capability() {
        let listener = ProxyListener {
            id: ListenerId::new(),
            ..ProxyListener::default()
        };
        let definition = RuleDefinition::create(
            RuleDefinitionDraft {
                name: "schema-free root".into(),
                enabled: true,
                priority: 1,
                listener_id: listener.id,
                stage: RuleStage::ProxyToUpstream,
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
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

        let program = compile_program(
            &workspace,
            &listener,
            None,
            false,
            RuleStage::ProxyToUpstream,
        )
        .unwrap();

        assert_eq!(program.rules().len(), 1);
    }
}
