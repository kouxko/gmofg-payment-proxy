use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc as std_mpsc},
};

use chrono::Utc;
use intercept_proxy_application::EventHub;
use intercept_proxy_domain::{
    MatchContext, MessageStage, NthCounterAdvance, NthCounterSnapshot, RuleConditionEvaluation,
    RuleEngine, RuleRuntimeSnapshot, RuntimeEpoch,
};
use intercept_proxy_exchange::SocketContext;
use intercept_proxy_runtime::{
    ConnectionContext, JointRuleConditionEvaluation, Result as ProxyResult, SocketJointEvaluation,
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::{
    EvaluatedRules, MAILBOX_CAPACITY, matched_rule_summaries, publish_repository_error,
    rule_trace_text,
};
use crate::adapters::pipeline::{
    JointDocumentEvaluation, RuntimeRuleRepository, app_to_proxy, domain_channel,
    rule_actions::terminal_identity,
};

pub(super) type RuleActorSender = mpsc::Sender<Command>;

#[derive(Debug)]
struct RuleRuntime {
    snapshot: RuleRuntimeSnapshot,
    engine: RuleEngine,
}

pub(super) struct EvaluationInput {
    pub(super) context: ConnectionContext,
    pub(super) stage: MessageStage,
    pub(super) json: Option<serde_json::Value>,
    pub(super) target: Option<String>,
    pub(super) joint_document: Option<JointDocumentEvaluation>,
    pub(super) socket_joint: Option<Box<dyn SocketJointEvaluation>>,
    pub(super) message: Option<intercept_proxy_runtime::Message>,
    pub(super) body_codec: Option<Arc<dyn intercept_proxy_product_api::BodyCodec>>,
}

pub(super) enum Reply {
    Async(oneshot::Sender<ProxyResult<EvaluatedRules>>),
    Handshake(std_mpsc::SyncSender<ProxyResult<EvaluatedRules>>),
}

impl Reply {
    fn send(self, result: ProxyResult<EvaluatedRules>) {
        match self {
            Self::Async(reply) => drop(reply.send(result)),
            Self::Handshake(reply) => drop(reply.send(result)),
        }
    }
}

pub(super) enum Command {
    Evaluate {
        input: Box<EvaluationInput>,
        reply: Reply,
    },
    Stop {
        completed: oneshot::Sender<()>,
    },
}

pub(super) fn spawn(
    epoch: Uuid,
    rules: Arc<dyn RuntimeRuleRepository>,
    events: Arc<EventHub>,
    channel_labels: Arc<BTreeMap<String, String>>,
) -> RuleActorSender {
    let (sender, receiver) = mpsc::channel(MAILBOX_CAPACITY);
    tokio::spawn(run(epoch, receiver, rules, events, channel_labels));
    sender
}

async fn run(
    epoch: Uuid,
    mut commands: mpsc::Receiver<Command>,
    rules: Arc<dyn RuntimeRuleRepository>,
    events: Arc<EventHub>,
    channel_labels: Arc<BTreeMap<String, String>>,
) {
    let mut runtime = None;
    while let Some(command) = commands.recv().await {
        match command {
            Command::Evaluate { mut input, reply } => {
                reply.send(
                    evaluate_owned(
                        epoch,
                        &mut runtime,
                        &mut input,
                        rules.as_ref(),
                        events.as_ref(),
                        channel_labels.as_ref(),
                    )
                    .await,
                );
            }
            Command::Stop { completed } => {
                if let Some(collection_id) = runtime
                    .take()
                    .and_then(|runtime: RuleRuntime| runtime.snapshot.collection_id)
                    && let Err(error) = rules.reset_runtime_hit_metadata(collection_id).await
                {
                    publish_repository_error(events.as_ref(), epoch, &error);
                }
                let _ = completed.send(());
                break;
            }
        }
    }
}

async fn evaluate_owned(
    epoch: Uuid,
    runtime: &mut Option<RuleRuntime>,
    input: &mut EvaluationInput,
    rules: &dyn RuntimeRuleRepository,
    events: &EventHub,
    channel_labels: &BTreeMap<String, String>,
) -> ProxyResult<EvaluatedRules> {
    prepare_runtime(runtime, input, rules).await?;
    let current = runtime.as_mut().expect("rule runtime was initialized");
    let checkpoint = current.engine.clone();
    let counters_before = checkpoint.nth_counter_snapshots();
    let terminal = terminal_identity(&input.context);
    let match_context = MatchContext {
        runtime_epoch: RuntimeEpoch::from_uuid(epoch),
        channel: domain_channel(&input.context.channel)?,
        stage: input.stage,
        terminal: &terminal,
        path_or_request_type: input.target.as_deref(),
        json_body: input.json.as_ref(),
    };
    let execution_order = current.snapshot.execution_order.clone();
    let evaluation = evaluate_rules(
        current,
        &mut input.joint_document,
        &mut input.socket_joint,
        &match_context,
        &execution_order,
    )?;
    let hit_rules = matched_rule_summaries(&evaluation, current.engine.rules(), channel_labels);
    let (prepared_message, prepared_socket, fault_actions, pause) =
        match prepare_evaluated_message(input, &evaluation, &hit_rules).await {
            Ok(prepared) => prepared,
            Err(error) => {
                current.engine = checkpoint;
                return Err(error);
            }
        };
    let nth_advances = match nth_advances(&counters_before, &current.engine.nth_counter_snapshots())
    {
        Ok(advances) => advances,
        Err(error) => {
            current.engine = checkpoint;
            return Err(app_to_proxy(error.into()));
        }
    };
    if evaluation.traces.iter().any(|trace| trace.matched) || !nth_advances.is_empty() {
        let base = current.snapshot.clone();
        let evaluated_rules = current.engine.rules().to_vec();
        let deltas = match crate::adapters::rules::conversion::runtime_deltas(
            &base,
            &evaluated_rules,
            &nth_advances,
        ) {
            Ok(deltas) => deltas,
            Err(error) => {
                current.engine = checkpoint;
                return Err(app_to_proxy(error));
            }
        };
        match rules.commit_runtime_deltas(&base, &deltas).await {
            Ok(revision) => {
                current.snapshot = RuleRuntimeSnapshot::with_collection_identity_and_order(
                    base.collection_id,
                    revision,
                    evaluated_rules,
                    base.execution_order,
                );
            }
            Err(error) => {
                current.engine = checkpoint;
                *runtime = None;
                publish_repository_error(events, epoch, &error);
                return Err(app_to_proxy(error));
            }
        }
    }
    let traces = rule_trace_text(&evaluation);
    let matched_ids = evaluation
        .traces
        .iter()
        .filter(|trace| trace.matched)
        .map(|trace| trace.rule_id.as_uuid())
        .collect();
    Ok(EvaluatedRules {
        actions: evaluation.composed_actions,
        traces,
        matched_ids,
        hit_rules,
        prepared_message,
        prepared_socket,
        fault_actions,
        pause,
    })
}

fn evaluate_rules(
    current: &mut RuleRuntime,
    joint_document: &mut Option<JointDocumentEvaluation>,
    socket_joint: &mut Option<Box<dyn SocketJointEvaluation>>,
    match_context: &MatchContext<'_>,
    execution_order: &[intercept_proxy_domain::RuleId],
) -> ProxyResult<intercept_proxy_domain::RuleEvaluation> {
    let evaluation = match (joint_document.as_mut(), socket_joint.as_mut()) {
        (Some(joint), None) => current
            .engine
            .evaluate_with_condition_gate_in_order(
                match_context,
                Utc::now(),
                execution_order,
                |rule, nth_attempt| {
                    joint
                        .gate(rule, nth_attempt, match_context)
                        .map(|evaluated| match evaluated {
                            JointRuleConditionEvaluation::UnifiedOwned(condition) => {
                                RuleConditionEvaluation::UnifiedOwned(
                                    intercept_proxy_domain::ConditionEvaluation {
                                        matched: condition.matched,
                                        eligible_without_nth: condition.eligible_without_nth,
                                        contains_nth: condition.contains_nth,
                                    },
                                )
                            }
                            JointRuleConditionEvaluation::NotOwned => {
                                RuleConditionEvaluation::NotOwned
                            }
                        })
                },
            )
            .map_err(|error| app_to_proxy(error.into()))?,
        (None, Some(joint)) => current.engine.evaluate_with_condition_gate_in_order(
            match_context,
            Utc::now(),
            execution_order,
            |rule, nth_attempt| {
                joint
                    .gate(rule.id.as_uuid(), nth_attempt)
                    .map(|evaluated| match evaluated {
                        JointRuleConditionEvaluation::UnifiedOwned(condition) => {
                            RuleConditionEvaluation::UnifiedOwned(
                                intercept_proxy_domain::ConditionEvaluation {
                                    matched: condition.matched,
                                    eligible_without_nth: condition.eligible_without_nth,
                                    contains_nth: condition.contains_nth,
                                },
                            )
                        }
                        JointRuleConditionEvaluation::NotOwned => RuleConditionEvaluation::NotOwned,
                    })
            },
        )?,
        (None, None) => current
            .engine
            .evaluate_with_gate_in_order(match_context, Utc::now(), execution_order, |_| {
                Ok::<_, std::convert::Infallible>(true)
            })
            .expect("an infallible rule gate cannot fail"),
        (Some(_), Some(_)) => unreachable!("one evaluation cannot be both HTTP and Socket"),
    };
    Ok(evaluation)
}

fn nth_advances(
    before: &[NthCounterSnapshot],
    after: &[NthCounterSnapshot],
) -> Result<Vec<NthCounterAdvance>, intercept_proxy_domain::DomainError> {
    let mut advances = Vec::new();
    for next in after {
        let expected_attempts = before
            .iter()
            .find(|previous| previous.rule_id == next.rule_id && previous.terminal == next.terminal)
            .map_or(0, |previous| previous.attempts);
        let increment = next
            .attempts
            .checked_sub(expected_attempts)
            .ok_or_else(|| {
                intercept_proxy_domain::DomainError::new(
                    intercept_proxy_domain::ErrorCode::RuleInvalid,
                    "Nth counter 不得减少",
                )
            })?;
        if increment > 0 {
            if increment != 1 {
                return Err(intercept_proxy_domain::DomainError::new(
                    intercept_proxy_domain::ErrorCode::RuleInvalid,
                    "Nth counter 每次事务只能增加 1",
                ));
            }
            advances.push(NthCounterAdvance {
                rule_id: next.rule_id,
                terminal: next.terminal.clone(),
                expected_attempts,
                increment,
            });
        }
    }
    Ok(advances)
}

async fn prepare_evaluated_message(
    input: &mut EvaluationInput,
    evaluation: &intercept_proxy_domain::RuleEvaluation,
    hit_rules: &[intercept_proxy_application::RuleSummaryViewModel],
) -> ProxyResult<(
    Option<intercept_proxy_runtime::Message>,
    Option<SocketContext>,
    Vec<intercept_proxy_runtime::FaultAction>,
    bool,
)> {
    let mut prepared_message = input.message.clone();
    let Some(message) = prepared_message.as_mut() else {
        let prepared_socket = match input.socket_joint.take() {
            Some(joint) => Some(joint.encode().await.map_err(|error| {
                let mut mapped = intercept_proxy_runtime::ProxyError::new(
                    intercept_proxy_runtime::ErrorCode::ExternalPackageCallFailed,
                    error.message,
                );
                mapped.external_package_call = error.external_package_call;
                mapped
            })?),
            None => None,
        };
        return Ok((prepared_message, prepared_socket, Vec::new(), false));
    };
    if let Some(joint) = input.joint_document.take() {
        joint.encode_into(message).await.map_err(|error| {
            intercept_proxy_runtime::ProxyError::new(
                intercept_proxy_runtime::ErrorCode::ExternalPackageCallFailed,
                format!("联合 Document Encode 失败：{}", error.message),
            )
            .with_external_package_call(error.external_package_call)
        })?;
    }
    let body_codec = input
        .body_codec
        .as_ref()
        .expect("message policy input always includes a body codec");
    let seed = crate::adapters::pipeline::rule_actions::weak_network_seed(
        &input.context,
        input.stage,
        hit_rules,
    );
    let (fault_actions, pause) = crate::adapters::pipeline::rule_actions::apply_rule_actions(
        body_codec.as_ref(),
        message,
        &evaluation.composed_actions,
        seed,
    )?;
    Ok((prepared_message, None, fault_actions, pause))
}

async fn prepare_runtime(
    runtime: &mut Option<RuleRuntime>,
    input: &EvaluationInput,
    rules: &dyn RuntimeRuleRepository,
) -> ProxyResult<()> {
    let mut snapshot = rules
        .runtime_snapshot(&input.context.channel)
        .await
        .map_err(app_to_proxy)?;
    if let Some(current) = runtime {
        if current.snapshot.collection_id != snapshot.collection_id
            || current.snapshot.collection_revision != snapshot.collection_revision
            || current.snapshot.signature != snapshot.signature
        {
            current.engine.reconcile(snapshot.rules.clone());
            current.snapshot = snapshot;
        }
        return Ok(());
    }
    if let Some(collection_id) = snapshot.collection_id {
        rules
            .reset_runtime_hit_metadata(collection_id)
            .await
            .map_err(app_to_proxy)?;
        snapshot = rules
            .runtime_snapshot(&input.context.channel)
            .await
            .map_err(app_to_proxy)?;
    }
    *runtime = Some(RuleRuntime {
        engine: RuleEngine::new(
            RuntimeEpoch::from_uuid(input.context.runtime_epoch),
            snapshot.rules.clone(),
        ),
        snapshot,
    });
    Ok(())
}
