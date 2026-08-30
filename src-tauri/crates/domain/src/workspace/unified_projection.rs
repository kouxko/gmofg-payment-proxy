use super::{
    Condition, ConditionTree, DocumentAction, DocumentCondition, DocumentMutation,
    DocumentPredicate, DomainError, ErrorCode, HttpDocumentRuleContent, MessageStage,
    ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId, ProtocolRuleStage, RuleAction,
    RuleContent, RuleDefinition, RuleStage, SocketRuleContent, UnifiedAction,
};
use crate::ProtocolPackageRef;

pub(super) fn restore_document_rule(
    definition: &RuleDefinition,
    content: HttpDocumentRuleContent,
) -> Result<ProtocolDocumentRuleDefinition, DomainError> {
    let (conditions, actions) = legacy_document_parts(definition)?;
    ProtocolDocumentRuleDefinition::restore_from_unified(
        ProtocolDocumentRuleId::from_uuid(definition.rule_id().as_uuid()),
        definition.revision(),
        definition.name().to_owned(),
        definition.enabled(),
        definition.priority(),
        definition.created_order(),
        definition.listener_id(),
        content.package,
        protocol_stage_from_rule(definition.stage())?,
        conditions,
        actions,
    )
}

pub(super) fn legacy_http_parts(
    content: &crate::HttpRuleContent,
) -> Result<(Vec<Condition>, Vec<RuleAction>), DomainError> {
    let mut conditions = Vec::new();
    collect_legacy_http_conditions(&content.condition, &mut conditions)?;
    let actions = content
        .actions
        .iter()
        .filter_map(|action| match action {
            UnifiedAction::Http(action) => Some(action.clone()),
            UnifiedAction::Terminal(action) => Some(RuleAction::Terminal(action.clone())),
            UnifiedAction::RecordMatch | UnifiedAction::Document(_) => None,
        })
        .collect();
    Ok((conditions, actions))
}

pub(super) fn actor_owned_socket_conditions(
    tree: &ConditionTree,
) -> Result<Vec<Condition>, DomainError> {
    let mut conditions = Vec::new();
    collect_actor_owned_socket_conditions(tree, &mut conditions)?;
    Ok(conditions)
}

fn collect_actor_owned_socket_conditions(
    tree: &ConditionTree,
    output: &mut Vec<Condition>,
) -> Result<(), DomainError> {
    match tree {
        ConditionTree::All(children) => {
            for child in children {
                collect_actor_owned_socket_conditions(child, output)?;
            }
            Ok(())
        }
        ConditionTree::Any(_) => Err(unified_persistence_error(
            "condition",
            "Socket actor runtime 尚不支持跨 owner OR 条件",
        )),
        ConditionTree::Leaf(Condition::NthHit { count }) => {
            output.push(Condition::NthHit { count: *count });
            Ok(())
        }
        ConditionTree::Leaf(Condition::Document { .. }) => Ok(()),
        ConditionTree::Leaf(Condition::Http { .. }) => Err(unified_persistence_error(
            "condition",
            "Socket actor runtime 不接受 HTTP 条件",
        )),
    }
}

fn collect_legacy_http_conditions(
    tree: &ConditionTree,
    output: &mut Vec<Condition>,
) -> Result<(), DomainError> {
    match tree {
        ConditionTree::All(children) => {
            for child in children {
                collect_legacy_http_conditions(child, output)?;
            }
            Ok(())
        }
        ConditionTree::Any(_) => Err(unified_persistence_error(
            "condition",
            "旧 HTTP runtime 尚不支持 OR；Phase 7 将切换统一执行",
        )),
        ConditionTree::Leaf(Condition::Http { condition }) => {
            output.push(Condition::Http {
                condition: condition.clone(),
            });
            Ok(())
        }
        ConditionTree::Leaf(Condition::Document { .. }) => Ok(()),
        ConditionTree::Leaf(Condition::NthHit { count }) => {
            output.push(Condition::NthHit { count: *count });
            Ok(())
        }
    }
}

fn legacy_document_parts(
    definition: &RuleDefinition,
) -> Result<(Vec<DocumentCondition>, Vec<DocumentAction>), DomainError> {
    let (tree, actions) = match definition.content() {
        RuleContent::Http(content) => (&content.condition, &content.actions),
        RuleContent::Socket(content) => (&content.condition, &content.actions),
    };
    let mut conditions = Vec::new();
    collect_legacy_document_conditions(tree, &mut conditions)?;
    let actions = actions
        .iter()
        .filter_map(|action| match action {
            UnifiedAction::Document(DocumentMutation::Set { path, value }) => {
                Some(Ok(DocumentAction::SetField {
                    field: path.clone(),
                    value: value.clone(),
                }))
            }
            UnifiedAction::Document(DocumentMutation::Clear { path }) => {
                Some(Ok(DocumentAction::ClearField {
                    field: path.clone(),
                }))
            }
            UnifiedAction::Document(
                DocumentMutation::Insert { .. } | DocumentMutation::Append { .. },
            ) => Some(Err(unified_persistence_error(
                "actions",
                "旧 Document runtime 尚不支持 Insert/Append；Phase 7 将切换统一执行",
            ))),
            UnifiedAction::RecordMatch => Some(Ok(DocumentAction::RecordMatch)),
            UnifiedAction::Http(_) | UnifiedAction::Terminal(_) => None,
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((conditions, actions))
}

fn collect_legacy_document_conditions(
    tree: &ConditionTree,
    output: &mut Vec<DocumentCondition>,
) -> Result<(), DomainError> {
    match tree {
        ConditionTree::All(children) => {
            for child in children {
                collect_legacy_document_conditions(child, output)?;
            }
            Ok(())
        }
        ConditionTree::Any(_) => Err(unified_persistence_error(
            "condition",
            "旧 Document runtime 尚不支持 OR；Phase 7 将切换统一执行",
        )),
        ConditionTree::Leaf(Condition::Document { path, predicate }) => {
            let value = match predicate {
                DocumentPredicate::String(value)
                    if value.operator == crate::StringOperator::Equal =>
                {
                    crate::DocumentValue::String(value.value.clone())
                }
                DocumentPredicate::Number(value)
                    if value.operator == crate::NumberOperator::Equal =>
                {
                    crate::DocumentValue::Number(value.value)
                }
                DocumentPredicate::Boolean(crate::BooleanPredicate::Equal(value)) => {
                    crate::DocumentValue::Boolean(*value)
                }
                DocumentPredicate::NullEqual => crate::DocumentValue::null(),
                _ => {
                    return Err(unified_persistence_error(
                        "condition",
                        "旧 Document runtime 尚不支持该 typed predicate；Phase 7 将切换统一执行",
                    ));
                }
            };
            output.push(DocumentCondition::Equals {
                field: path.clone(),
                value,
            });
            Ok(())
        }
        ConditionTree::Leaf(Condition::Http { .. } | Condition::NthHit { .. }) => Ok(()),
    }
}

pub(super) fn unified_http_tree(conditions: Vec<Condition>) -> ConditionTree {
    ConditionTree::All(conditions.into_iter().map(ConditionTree::Leaf).collect())
}

pub(super) fn unified_http_actions(actions: Vec<RuleAction>) -> Vec<UnifiedAction> {
    actions
        .into_iter()
        .map(|action| match action {
            RuleAction::Terminal(action) => UnifiedAction::Terminal(action),
            action => UnifiedAction::Http(action),
        })
        .collect()
}

pub(super) fn unified_socket_content(
    rule: &ProtocolDocumentRuleDefinition,
    package: ProtocolPackageRef,
) -> RuleContent {
    let condition = ConditionTree::All(
        rule.conditions()
            .iter()
            .map(|condition| match condition {
                DocumentCondition::Equals { field, value } => {
                    ConditionTree::Leaf(Condition::Document {
                        path: field.clone(),
                        predicate: match value {
                            crate::DocumentValue::String(value) => {
                                DocumentPredicate::String(crate::StringPredicate {
                                    operator: crate::StringOperator::Equal,
                                    value: value.clone(),
                                })
                            }
                            crate::DocumentValue::Number(value) => {
                                DocumentPredicate::Number(crate::NumberPredicate {
                                    operator: crate::NumberOperator::Equal,
                                    value: *value,
                                })
                            }
                            crate::DocumentValue::Boolean(value) => {
                                DocumentPredicate::Boolean(crate::BooleanPredicate::Equal(*value))
                            }
                            crate::DocumentValue::Null(()) => DocumentPredicate::NullEqual,
                            crate::DocumentValue::Object(_) | crate::DocumentValue::Array(_) => {
                                return ConditionTree::All(Vec::new());
                            }
                        },
                    })
                }
            })
            .collect(),
    );
    let actions = rule
        .actions()
        .iter()
        .map(|action| match action {
            DocumentAction::SetField { field, value } => {
                UnifiedAction::Document(DocumentMutation::Set {
                    path: field.clone(),
                    value: value.clone(),
                })
            }
            DocumentAction::ClearField { field } => {
                UnifiedAction::Document(DocumentMutation::Clear {
                    path: field.clone(),
                })
            }
            DocumentAction::RecordMatch => UnifiedAction::RecordMatch,
        })
        .collect();
    RuleContent::Socket(SocketRuleContent {
        package,
        condition,
        actions,
    })
}

pub(super) const fn message_stage_from_rule(stage: RuleStage) -> MessageStage {
    match stage {
        RuleStage::AppToProxy | RuleStage::ProxyToUpstream => MessageStage::Request,
        RuleStage::UpstreamToProxy | RuleStage::ProxyToApp => MessageStage::Response,
        RuleStage::TlsHandshake => MessageStage::TlsHandshake,
    }
}

pub(super) fn runtime_priority(priority: i32) -> Result<u32, DomainError> {
    u32::try_from(priority)
        .map_err(|_| unified_persistence_error("priority", "HTTP 规则 priority 必须是非负整数"))
}

fn protocol_stage_from_rule(stage: RuleStage) -> Result<ProtocolRuleStage, DomainError> {
    match stage {
        RuleStage::AppToProxy => Ok(ProtocolRuleStage::AppToProxy),
        RuleStage::ProxyToUpstream => Ok(ProtocolRuleStage::ProxyToUpstream),
        RuleStage::UpstreamToProxy => Ok(ProtocolRuleStage::UpstreamToProxy),
        RuleStage::ProxyToApp => Ok(ProtocolRuleStage::ProxyToApp),
        RuleStage::TlsHandshake => Err(DomainError::new(
            ErrorCode::RuleInvalid,
            "Document 规则不能使用 TLS 握手阶段",
        )),
    }
}

pub(super) const fn rule_stage_from_protocol(stage: ProtocolRuleStage) -> RuleStage {
    match stage {
        ProtocolRuleStage::AppToProxy => RuleStage::AppToProxy,
        ProtocolRuleStage::ProxyToUpstream => RuleStage::ProxyToUpstream,
        ProtocolRuleStage::UpstreamToProxy => RuleStage::UpstreamToProxy,
        ProtocolRuleStage::ProxyToApp => RuleStage::ProxyToApp,
    }
}

pub(super) fn unified_persistence_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "统一规则持久化数据无效")
        .with_field_error(field, message)
}
