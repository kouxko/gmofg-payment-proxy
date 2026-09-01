use chrono::Utc;
use intercept_proxy_domain::{
    HttpAction, HttpHeader, MatchContext, RuleContent, RuleDefinition, RuleEvaluation, RuleId,
    RuleLifecycleDelta, RuleTrace, UnifiedAction,
};
use intercept_proxy_runtime::{
    ErrorCode, JointConditionEvaluation, JointRuleConditionEvaluation, Message, ProxyError,
    Result as ProxyResult, SocketJointEvaluation,
};

use super::{RuleRuntime, message_stage};
use crate::adapters::pipeline::{JointDocumentEvaluation, app_to_proxy};

pub(super) fn evaluate_rules(
    current: &mut RuleRuntime,
    joint_document: &mut Option<JointDocumentEvaluation>,
    socket_joint: &mut Option<Box<dyn SocketJointEvaluation>>,
    context: &MatchContext<'_>,
    execution_order: &[RuleId],
    mut message: Option<&mut Message>,
    body_codec: Option<&dyn intercept_proxy_product_api::BodyCodec>,
) -> ProxyResult<(RuleEvaluation, Vec<RuleLifecycleDelta>)> {
    let rules = ordered_rules(&current.snapshot.rules, execution_order);
    let mut evaluation = RuleEvaluation::default();
    let mut deltas = Vec::new();
    for rule in rules.iter().filter(|rule| rule.enabled()) {
        if !belongs_to_stage(rule, context) {
            continue;
        }
        let condition = evaluate_condition(
            rule,
            joint_document,
            socket_joint,
            context,
            message.as_deref_mut(),
            body_codec,
        )?;
        if !condition.matched {
            record_miss(rule, &mut evaluation);
            continue;
        }
        deltas.push(rule.lifecycle_delta_for_successful_match(Utc::now()));
        execute_actions(
            rule,
            joint_document.is_some(),
            socket_joint.is_some(),
            &mut message,
            body_codec,
            &mut evaluation,
        )?;
        if evaluation.terminal_action.is_some() {
            break;
        }
    }
    Ok((evaluation, deltas))
}

fn ordered_rules(rules: &[RuleDefinition], execution_order: &[RuleId]) -> Vec<RuleDefinition> {
    let order = execution_order
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index))
        .collect::<std::collections::HashMap<_, _>>();
    let mut rules = rules.to_vec();
    rules.sort_by_key(|rule| {
        (
            order.get(&rule.rule_id()).copied().unwrap_or(usize::MAX),
            rule.priority(),
            rule.rule_id(),
        )
    });
    rules
}

fn belongs_to_stage(rule: &RuleDefinition, context: &MatchContext<'_>) -> bool {
    rule.listener_id().to_string() == context.channel.as_str()
        && message_stage(rule.stage()) == context.stage
}

fn evaluate_condition(
    rule: &RuleDefinition,
    joint_document: &mut Option<JointDocumentEvaluation>,
    socket_joint: &mut Option<Box<dyn SocketJointEvaluation>>,
    context: &MatchContext<'_>,
    message: Option<&mut Message>,
    body_codec: Option<&dyn intercept_proxy_product_api::BodyCodec>,
) -> ProxyResult<JointConditionEvaluation> {
    let owned = match (
        rule.content(),
        joint_document.as_mut(),
        socket_joint.as_mut(),
    ) {
        (RuleContent::Http(_), Some(joint), None) => joint
            .gate(
                rule.rule_id(),
                context,
                message.expect("HTTP unified evaluation owns a working message"),
                body_codec.expect("HTTP unified evaluation owns its selected body codec"),
            )
            .map_err(|error| app_to_proxy(error.into()))?,
        (RuleContent::Socket(_), None, Some(joint)) => joint.gate(rule.rule_id().as_uuid())?,
        (RuleContent::Http(http_rule), None, None) => {
            ordinary_http_condition(http_rule, context, message.as_deref())?
        }
        (RuleContent::Socket(_), _, _) => {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "Socket unified rule requires its typed Document transaction",
            ));
        }
        (RuleContent::Http(_), None, Some(_)) => {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "HTTP unified rule cannot use a Socket Document transaction",
            ));
        }
        (_, Some(_), Some(_)) => unreachable!("one evaluation cannot be both HTTP and Socket"),
    };
    match owned {
        JointRuleConditionEvaluation::UnifiedOwned(condition) => Ok(condition),
        JointRuleConditionEvaluation::NotOwned => Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "unified runtime program does not own its RuleDefinition",
        )),
    }
}

fn ordinary_http_condition(
    http_rule: &intercept_proxy_domain::HttpRuleContent,
    match_context: &MatchContext<'_>,
    message: Option<&Message>,
) -> ProxyResult<JointRuleConditionEvaluation> {
    let values = message
        .map(|message| {
            message
                .headers
                .iter()
                .map(|header| (header.name.clone(), header.value.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let headers = values
        .iter()
        .map(|(name, value)| HttpHeader::new(name, value))
        .collect::<Vec<_>>();
    let current = MatchContext {
        headers: &headers,
        ..match_context.clone()
    };
    let condition =
        intercept_proxy_domain::evaluate_http_context_condition(&http_rule.condition, &current)
            .map_err(|error| app_to_proxy(error.into()))?;
    Ok(JointRuleConditionEvaluation::UnifiedOwned(
        JointConditionEvaluation {
            matched: condition.matched,
        },
    ))
}

fn record_miss(rule: &RuleDefinition, evaluation: &mut RuleEvaluation) {
    evaluation.traces.push(RuleTrace {
        rule_id: rule.rule_id(),
        matched: false,
        reason: "统一条件树不满足".into(),
        actions: Vec::new(),
    });
}

fn execute_actions(
    rule: &RuleDefinition,
    has_joint_document: bool,
    has_socket_joint: bool,
    message: &mut Option<&mut Message>,
    body_codec: Option<&dyn intercept_proxy_product_api::BodyCodec>,
    evaluation: &mut RuleEvaluation,
) -> ProxyResult<()> {
    let mut executed = Vec::new();
    let action = rule_action(rule);
    match action {
        UnifiedAction::Http(
            action @ (HttpAction::Delay { .. }
            | HttpAction::Jitter { .. }
            | HttpAction::Throttle { .. }
            | HttpAction::Intermittent { .. }
            | HttpAction::CustomHttpStatus { .. }),
        ) => {
            executed.push(action.clone());
            evaluation.composed_actions.push(action.clone());
        }
        UnifiedAction::Terminal(terminal) => {
            let action = HttpAction::Terminal(terminal.clone());
            executed.push(action.clone());
            evaluation.composed_actions.push(action);
            evaluation.terminal_action = Some(terminal.clone());
        }
        UnifiedAction::Http(
            action @ (HttpAction::SetJsonField { .. }
            | HttpAction::ReplaceBodyText(_)
            | HttpAction::SetHeader { .. }),
        ) if !has_joint_document => {
            crate::adapters::pipeline::rule_actions::apply_rule_actions(
                body_codec.expect("HTTP message owns its codec"),
                message
                    .as_deref_mut()
                    .expect("HTTP unified evaluation owns a working message"),
                std::slice::from_ref(action),
                0,
            )?;
        }
        UnifiedAction::Document(_) if !has_joint_document && !has_socket_joint => {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "Document action requires its typed Document transaction",
            ));
        }
        UnifiedAction::RecordMatch | UnifiedAction::Document(_) | UnifiedAction::Http(_) => {}
    }
    evaluation.traces.push(RuleTrace {
        rule_id: rule.rule_id(),
        matched: true,
        reason: "统一条件树满足".into(),
        actions: executed,
    });
    Ok(())
}

fn rule_action(rule: &RuleDefinition) -> &UnifiedAction {
    match rule.content() {
        RuleContent::Http(content) => &content.action,
        RuleContent::Socket(content) => &content.action,
    }
}
