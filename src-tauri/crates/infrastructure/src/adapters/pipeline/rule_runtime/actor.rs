use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc as std_mpsc},
};

use intercept_proxy_application::EventHub;
use intercept_proxy_domain::{
    HttpHeader, MatchContext, MessageStage, RuleId, RuleRuntimeSnapshot, RuleStage, RuntimeEpoch,
};
use intercept_proxy_exchange::SocketContext;
use intercept_proxy_runtime::{ConnectionContext, Result as ProxyResult, SocketJointEvaluation};
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

mod evaluation;

pub(super) type RuleActorSender = mpsc::Sender<Command>;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CounterKey {
    rule_id: RuleId,
    source_ip: String,
    certificate_sha256: String,
}

#[derive(Clone, Debug)]
struct RuleRuntime {
    snapshot: RuleRuntimeSnapshot,
    counters: std::collections::HashMap<CounterKey, u64>,
}

pub(super) struct EvaluationInput {
    pub(super) context: ConnectionContext,
    pub(super) stage: MessageStage,
    pub(super) method: Option<String>,
    pub(super) request_target: Option<String>,
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
    let checkpoint = current.clone();
    let terminal = terminal_identity(&input.context);
    let http_header_values = input
        .message
        .as_ref()
        .map(|message| {
            message
                .headers
                .iter()
                .map(|header| (header.name.clone(), header.value.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let http_headers = http_header_values
        .iter()
        .map(|(name, value)| HttpHeader::new(name, value))
        .collect::<Vec<_>>();
    let match_context = MatchContext {
        runtime_epoch: RuntimeEpoch::from_uuid(epoch),
        channel: domain_channel(&input.context.channel)?,
        stage: input.stage,
        terminal: &terminal,
        method: input.method.as_deref(),
        request_target: input.request_target.as_deref(),
        headers: &http_headers,
    };
    let execution_order = current.snapshot.execution_order.clone();
    let (evaluation, deltas) = evaluation::evaluate_rules(
        current,
        &mut input.joint_document,
        &mut input.socket_joint,
        &match_context,
        &execution_order,
        input.message.as_mut(),
        input.body_codec.as_deref(),
    )?;
    let hit_rules = matched_rule_summaries(&evaluation, &current.snapshot.rules, channel_labels);
    let (prepared_message, prepared_socket, fault_actions, pause) =
        match prepare_evaluated_message(input, &evaluation, &hit_rules).await {
            Ok(prepared) => prepared,
            Err(error) => {
                *current = checkpoint;
                return Err(error);
            }
        };
    if !deltas.is_empty() {
        let base = current.snapshot.clone();
        let evaluated_rules =
            match crate::adapters::rules::conversion::apply_runtime_deltas(&base, &deltas) {
                Ok(rules) => rules,
                Err(error) => {
                    *current = checkpoint;
                    *runtime = None;
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
                *current = checkpoint;
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
            current.counters.retain(|key, _| {
                let previous = current
                    .snapshot
                    .rules
                    .iter()
                    .find(|rule| rule.rule_id() == key.rule_id);
                let replacement = snapshot
                    .rules
                    .iter()
                    .find(|rule| rule.rule_id() == key.rule_id);
                matches!((previous, replacement), (Some(previous), Some(replacement)) if replacement.enabled() && previous.to_draft() == replacement.to_draft())
            });
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
        snapshot,
        counters: std::collections::HashMap::new(),
    });
    Ok(())
}

const fn message_stage(stage: RuleStage) -> MessageStage {
    match stage {
        RuleStage::ProxyToUpstream => MessageStage::Request,
        RuleStage::ProxyToApp => MessageStage::Response,
        RuleStage::TlsHandshake => MessageStage::TlsHandshake,
    }
}
