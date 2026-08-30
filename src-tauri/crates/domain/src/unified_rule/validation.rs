use crate::{
    Condition, ConditionTree, DomainError, MessageStage, RuleAction, RuleDraft, UnifiedAction,
    validate_rule_draft,
};

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
            UnifiedAction::Terminal(action) => Some(RuleAction::Terminal(action.clone())),
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
        RuleStage::AppToProxy | RuleStage::UpstreamToProxy => {
            return Err(rule_binding_error(
                "stage",
                "该处理阶段只支持 Document 条件和动作，不支持普通 HTTP 条件或动作",
            ));
        }
    };
    let priority = u32::try_from(definition.priority)
        .map_err(|_| rule_binding_error("priority", "HTTP 规则优先级不能为负数"))?;
    validate_rule_draft(&RuleDraft {
        expected_revision: Some(definition.revision),
        name: definition.name.clone(),
        description: content.description.clone(),
        enabled: definition.enabled,
        priority,
        created_order: definition.created_order,
        channel: None,
        stage,
        conditions: conditions
            .into_iter()
            .map(|condition| Condition::Http { condition })
            .collect(),
        actions,
        one_shot: definition.one_shot,
    })
}

fn collect_http_conditions(tree: &ConditionTree, output: &mut Vec<crate::MatchCondition>) {
    match tree {
        ConditionTree::All(children) | ConditionTree::Any(children) => {
            for child in children {
                collect_http_conditions(child, output);
            }
        }
        ConditionTree::Leaf(Condition::Http { condition }) => output.push(condition.clone()),
        ConditionTree::Leaf(Condition::Document { .. } | Condition::NthHit { .. }) => {}
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
