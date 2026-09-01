use crate::{Condition, DomainError, HttpAction, MessageStage, UnifiedAction};

use super::{HttpRuleContent, RuleDefinition, RuleStage, rule_binding_error};

pub(super) fn validate_http_runtime_content(
    definition: &RuleDefinition,
    content: &HttpRuleContent,
) -> Result<(), DomainError> {
    let conditions = content
        .conditions
        .iter()
        .filter(|condition| matches!(condition, Condition::Http { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let actions = content
        .actions
        .iter()
        .filter_map(|action| match action {
            UnifiedAction::Http(action) => Some(action.clone()),
            UnifiedAction::Terminal(action) => Some(HttpAction::Terminal(action.clone())),
            UnifiedAction::RecordMatch | UnifiedAction::Document(_) => None,
        })
        .collect::<Vec<_>>();
    if conditions.is_empty() && actions.is_empty() {
        return Ok(());
    }
    let stage = match definition.stage {
        RuleStage::ProxyToUpstream => MessageStage::Request,
        RuleStage::ProxyToApp => MessageStage::Response,
    };
    crate::validate_http_rule(stage, &content.conditions, &actions)
}

pub(super) fn ensure_socket_only(
    conditions: &[Condition],
    actions: &[UnifiedAction],
) -> Result<(), DomainError> {
    if conditions
        .iter()
        .any(|condition| matches!(condition, Condition::Http { .. }))
        || actions
            .iter()
            .any(|action| matches!(action, UnifiedAction::Http(_) | UnifiedAction::Terminal(_)))
    {
        return Err(rule_binding_error(
            "content",
            "Socket 规则不能包含 HTTP 条件、HTTP 动作或尚未声明的终止动作",
        ));
    }
    Ok(())
}
