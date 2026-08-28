use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc as std_mpsc},
};

use chrono::Utc;
use intercept_proxy_application::EventHub;
use intercept_proxy_domain::{
    MatchContext, MessageStage, RuleEngine, RuleRuntimeSnapshot, RuntimeEpoch,
};
use intercept_proxy_runtime::{ConnectionContext, Result as ProxyResult};
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
    let joint_checkpoint = input.joint_document.clone();
    for remaining_retries in (0..=3).rev() {
        input.joint_document.clone_from(&joint_checkpoint);
        prepare_runtime(runtime, input, rules).await?;
        let current = runtime.as_mut().expect("rule runtime was initialized");
        let checkpoint = current.engine.clone();
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
        let evaluation = match input.joint_document.as_mut() {
            Some(joint) => current
                .engine
                .evaluate_with_gate_in_order(&match_context, Utc::now(), &execution_order, |rule| {
                    joint.gate(rule)
                })
                .map_err(|error| app_to_proxy(error.into()))?,
            None => current
                .engine
                .evaluate_with_gate_in_order(&match_context, Utc::now(), &execution_order, |_| {
                    Ok::<_, std::convert::Infallible>(true)
                })
                .expect("an infallible rule gate cannot fail"),
        };
        let hit_rules = matched_rule_summaries(&evaluation, current.engine.rules(), channel_labels);
        let (prepared_message, fault_actions, pause) =
            match prepare_evaluated_message(input, &evaluation, &hit_rules).await {
                Ok(prepared) => prepared,
                Err(error) => {
                    current.engine = checkpoint;
                    return Err(error);
                }
            };
        if evaluation.traces.iter().any(|trace| trace.matched) {
            let base = current.snapshot.clone();
            let evaluated_rules = current.engine.rules().to_vec();
            match rules.commit_runtime_snapshot(&base, &evaluated_rules).await {
                Ok(revision) => {
                    current.snapshot = RuleRuntimeSnapshot::with_collection_identity_and_order(
                        base.collection_id,
                        revision,
                        evaluated_rules,
                        base.execution_order,
                    );
                }
                Err(error)
                    if error.view_model.code == "REVISION_CONFLICT" && remaining_retries > 0 =>
                {
                    current.engine = checkpoint;
                    continue;
                }
                Err(error) => {
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
        return Ok(EvaluatedRules {
            actions: evaluation.composed_actions,
            traces,
            matched_ids,
            hit_rules,
            prepared_message,
            fault_actions,
            pause,
        });
    }
    unreachable!("retry loop always returns")
}

async fn prepare_evaluated_message(
    input: &mut EvaluationInput,
    evaluation: &intercept_proxy_domain::RuleEvaluation,
    hit_rules: &[intercept_proxy_application::RuleSummaryViewModel],
) -> ProxyResult<(
    Option<intercept_proxy_runtime::Message>,
    Vec<intercept_proxy_runtime::FaultAction>,
    bool,
)> {
    let mut prepared_message = input.message.clone();
    let Some(message) = prepared_message.as_mut() else {
        return Ok((prepared_message, Vec::new(), false));
    };
    if let Some(joint) = input.joint_document.take() {
        joint.encode_into(message).await.map_err(|error| {
            intercept_proxy_runtime::ProxyError::new(
                intercept_proxy_runtime::ErrorCode::Internal,
                format!("联合 Document Encode 失败：{error}"),
            )
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
    Ok((prepared_message, fault_actions, pause))
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
