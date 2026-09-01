use chrono::Utc;
use intercept_proxy_domain::{
    HttpAction, HttpHeader, MatchContext, RuleContent, RuleDefinition, RuleEvaluation, RuleId,
    RuleLifecycleDelta, RuleTrace, UnifiedAction,
};
use intercept_proxy_runtime::{
    ErrorCode, JointConditionEvaluation, JointRuleConditionEvaluation, Message, ProxyError,
    Result as ProxyResult, SocketJointEvaluation,
};

use super::{CounterKey, RuleRuntime, message_stage};
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
        let (key, expected_attempts, nth_attempt) = attempt(current, rule, context);
        let condition = evaluate_condition(
            rule,
            nth_attempt,
            joint_document,
            socket_joint,
            context,
            message.as_deref_mut(),
            body_codec,
        )?;
        let nth_advance = (condition.contains_nth && condition.eligible_without_nth).then(|| {
            current.counters.insert(key, nth_attempt);
            intercept_proxy_domain::NthCounterAdvance {
                rule_id: rule.rule_id(),
                terminal: context.terminal.clone(),
                expected_attempts,
                increment: 1,
            }
        });
        if !condition.matched {
            record_miss(rule, nth_advance, &mut evaluation, &mut deltas);
            continue;
        }
        let mut delta = rule.lifecycle_delta_for_successful_match(Utc::now());
        delta.nth_counter_advance = nth_advance;
        deltas.push(delta);
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

fn attempt(
    current: &RuleRuntime,
    rule: &RuleDefinition,
    context: &MatchContext<'_>,
) -> (CounterKey, u64, u64) {
    let key = CounterKey {
        rule_id: rule.rule_id(),
        source_ip: context.terminal.source_ip.clone(),
        certificate_sha256: context.terminal.certificate_sha256.clone(),
    };
    let expected = current.counters.get(&key).copied().unwrap_or_default();
    (key, expected, expected.saturating_add(1))
}

fn evaluate_condition(
    rule: &RuleDefinition,
    nth_attempt: u64,
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
                nth_attempt,
                context,
                message.expect("HTTP unified evaluation owns a working message"),
                body_codec.expect("HTTP unified evaluation owns its selected body codec"),
            )
            .map_err(|error| app_to_proxy(error.into()))?,
        (RuleContent::Socket(_), None, Some(joint)) => {
            joint.gate(rule.rule_id().as_uuid(), nth_attempt)?
        }
        (RuleContent::Http(http_rule), None, None) => {
            ordinary_http_condition(http_rule, nth_attempt, context, message.as_deref())?
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
    nth_attempt: u64,
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
    let condition = intercept_proxy_domain::evaluate_http_context_conditions_with_nth(
        &http_rule.conditions,
        nth_attempt,
        &current,
    )
    .map_err(|error| app_to_proxy(error.into()))?;
    Ok(JointRuleConditionEvaluation::UnifiedOwned(
        JointConditionEvaluation {
            matched: condition.matched,
            eligible_without_nth: condition.eligible_without_nth,
            contains_nth: condition.contains_nth,
        },
    ))
}

fn record_miss(
    rule: &RuleDefinition,
    nth_advance: Option<intercept_proxy_domain::NthCounterAdvance>,
    evaluation: &mut RuleEvaluation,
    deltas: &mut Vec<RuleLifecycleDelta>,
) {
    if let Some(nth_counter_advance) = nth_advance {
        deltas.push(RuleLifecycleDelta {
            rule_id: rule.rule_id(),
            expected_revision: rule.revision(),
            hit_count_increment: 0,
            last_hit_at: None,
            disable_one_shot: false,
            nth_counter_advance: Some(nth_counter_advance),
        });
    }
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
    for action in rule_actions(rule) {
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
                break;
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
    }
    evaluation.traces.push(RuleTrace {
        rule_id: rule.rule_id(),
        matched: true,
        reason: "统一条件树满足".into(),
        actions: executed,
    });
    Ok(())
}

fn rule_actions(rule: &RuleDefinition) -> &[UnifiedAction] {
    match rule.content() {
        RuleContent::Http(content) => &content.actions,
        RuleContent::Socket(content) => &content.actions,
    }
}
