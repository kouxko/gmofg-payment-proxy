use crate::{Condition, DomainError, HttpAction, MessageStage, UnifiedAction};

use super::{HttpRuleContent, RuleDefinition, RuleStage, rule_binding_error};

pub(super) fn validate_http_runtime_content(
    definition: &RuleDefinition,
    content: &HttpRuleContent,
) -> Result<(), DomainError> {
    let Condition::Http { .. } = &content.condition else {
        return Ok(());
    };
    let action = match &content.action {
        UnifiedAction::Http(action) => action.clone(),
        UnifiedAction::Terminal(action) => HttpAction::Terminal(action.clone()),
        UnifiedAction::RecordMatch | UnifiedAction::Document(_) => return Ok(()),
    };
    let stage = match definition.stage {
        RuleStage::ProxyToUpstream => MessageStage::Request,
        RuleStage::ProxyToApp => MessageStage::Response,
    };
    crate::validate_http_rule(stage, &content.condition, &action)
}

pub(super) fn ensure_socket_only(
    condition: &Condition,
    action: &UnifiedAction,
) -> Result<(), DomainError> {
    if matches!(condition, Condition::Http { .. })
        || matches!(action, UnifiedAction::Http(_) | UnifiedAction::Terminal(_))
    {
        return Err(rule_binding_error(
            "content",
            "Socket 规则不能包含 HTTP 条件、HTTP 动作或尚未声明的终止动作",
        ));
    }
    Ok(())
}
