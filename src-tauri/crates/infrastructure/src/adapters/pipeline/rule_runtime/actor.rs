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
    RuntimeRuleRepository, app_to_proxy, domain_channel, rule_actions::terminal_identity,
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
            Command::Evaluate { input, reply } => {
                reply.send(
                    evaluate_owned(
                        epoch,
                        &mut runtime,
                        &input,
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
    input: &EvaluationInput,
    rules: &dyn RuntimeRuleRepository,
    events: &EventHub,
    channel_labels: &BTreeMap<String, String>,
) -> ProxyResult<EvaluatedRules> {
    for remaining_retries in (0..=3).rev() {
        prepare_runtime(runtime, input, rules).await?;
        let current = runtime.as_mut().expect("rule runtime was initialized");
        let checkpoint = current.engine.clone();
        let terminal = terminal_identity(&input.context);
        let evaluation = current.engine.evaluate(
            &MatchContext {
                runtime_epoch: RuntimeEpoch::from_uuid(epoch),
                channel: domain_channel(&input.context.channel)?,
                stage: input.stage,
                terminal: &terminal,
                path_or_request_type: input.target.as_deref(),
                json_body: input.json.as_ref(),
            },
            Utc::now(),
        );
        let hit_rules = matched_rule_summaries(&evaluation, current.engine.rules(), channel_labels);
        if evaluation.traces.iter().any(|trace| trace.matched) {
            let base = current.snapshot.clone();
            let evaluated_rules = current.engine.rules().to_vec();
            match rules.commit_runtime_snapshot(&base, &evaluated_rules).await {
                Ok(revision) => {
                    current.snapshot = RuleRuntimeSnapshot::with_collection_identity(
                        base.collection_id,
                        revision,
                        evaluated_rules,
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
        });
    }
    unreachable!("retry loop always returns")
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
