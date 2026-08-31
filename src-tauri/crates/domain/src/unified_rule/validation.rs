use crate::{Condition, ConditionTree, DomainError, HttpAction, MessageStage, UnifiedAction};

use super::{HttpRuleContent, RuleDefinition, RuleStage, rule_binding_error};

pub(super) fn validate_http_runtime_content(
    definition: &RuleDefinition,
    content: &HttpRuleContent,
) -> Result<(), DomainError> {
    let mut conditions = Vec::new();
    collect_http_conditions(&content.condition, &mut conditions);
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
        RuleStage::TlsHandshake => MessageStage::TlsHandshake,
    };
    crate::validate_http_rule(stage, &content.condition, &actions)
}

fn collect_http_conditions(tree: &ConditionTree, output: &mut Vec<Condition>) {
    match tree {
        ConditionTree::All(children) | ConditionTree::Any(children) => {
            for child in children {
                collect_http_conditions(child, output);
            }
        }
        ConditionTree::Leaf(condition @ Condition::Http { .. }) => output.push(condition.clone()),
        ConditionTree::Leaf(
            Condition::Document { .. }
            | Condition::DocumentPattern { .. }
            | Condition::NthHit { .. },
        ) => {}
    }
}

pub(super) fn ensure_socket_only(
    tree: &ConditionTree,
    actions: &[UnifiedAction],
) -> Result<(), DomainError> {
    let mut http = Vec::new();
    collect_http_conditions(tree, &mut http);
    if !http.is_empty()
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
