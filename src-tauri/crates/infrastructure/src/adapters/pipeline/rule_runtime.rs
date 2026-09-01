//! Epoch-owned rule actors serialize evaluation and durable metadata commits.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use chrono::Utc;
use intercept_proxy_application::{
    AppError, EventHub, RuleSummaryViewModel, UiEventPayload, UiTone,
};
use intercept_proxy_domain::{MessageStage, RuleDefinition, RuleStage};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::http::HttpRequestMetadata;
use intercept_proxy_runtime::{
    ConnectionContext, ErrorCode, Message, ProxyError, Result as ProxyResult,
};
use parking_lot::Mutex;
use tokio::sync::oneshot;
use uuid::Uuid;

mod actor;

use actor::{Command, EvaluationInput, Reply, RuleActorSender};

use super::{JointDocumentEvaluation, RuntimeRuleRepository};

pub(super) const MAILBOX_CAPACITY: usize = 64;
#[derive(Debug)]
pub(super) struct RuleRuntimeService {
    channel_labels: Arc<BTreeMap<String, String>>,
    rules: Arc<dyn RuntimeRuleRepository>,
    events: Arc<EventHub>,
    actors: Mutex<ActorRegistry>,
}

#[derive(Debug, Default)]
struct ActorRegistry {
    senders: BTreeMap<Uuid, RuleActorSender>,
    active_epochs: BTreeSet<Uuid>,
}

#[derive(Debug)]
pub(super) struct EvaluatedRules {
    pub(super) traces: Vec<String>,
    pub(super) matched_ids: Vec<Uuid>,
    pub(super) hit_rules: Vec<RuleSummaryViewModel>,
    pub(super) prepared_message: Option<Message>,
    pub(super) prepared_socket: Option<intercept_proxy_exchange::SocketContext>,
    pub(super) fault_actions: Vec<intercept_proxy_runtime::FaultAction>,
}

impl RuleRuntimeService {
    pub(super) fn new(
        channel_labels: BTreeMap<String, String>,
        rules: Arc<dyn RuntimeRuleRepository>,
        events: Arc<EventHub>,
    ) -> Self {
        Self {
            channel_labels: Arc::new(channel_labels),
            rules,
            events,
            actors: Mutex::new(ActorRegistry::default()),
        }
    }

    pub(super) fn prepare_epoch(&self, epoch: Uuid) -> ProxyResult<()> {
        self.sender_for(epoch).map(|_| ())
    }

    pub(super) fn runtime_started(&self, epoch: Uuid) -> ProxyResult<()> {
        self.actors.lock().active_epochs.insert(epoch);
        self.sender_for(epoch).map(|_| ())
    }

    pub(super) async fn evaluate(
        &self,
        context: &ConnectionContext,
        stage: MessageStage,
        request: &HttpRequestMetadata,
        message: Option<&Message>,
        body_codec: Arc<dyn BodyCodec>,
        joint_document: Option<JointDocumentEvaluation>,
    ) -> ProxyResult<EvaluatedRules> {
        let input = EvaluationInput {
            context: context.clone(),
            stage,
            method: Some(request.method.clone()),
            request_target: Some(request.request_target.clone()),
            joint_document,
            socket_joint: None,
            message: message.cloned(),
            body_codec: Some(body_codec),
        };
        self.submit(input, None).await
    }

    pub(super) async fn evaluate_socket(
        &self,
        context: &ConnectionContext,
        stage: MessageStage,
        joint: Box<dyn intercept_proxy_runtime::SocketJointEvaluation>,
    ) -> ProxyResult<EvaluatedRules> {
        self.submit(
            EvaluationInput {
                context: context.clone(),
                stage,
                method: None,
                request_target: None,
                joint_document: None,
                socket_joint: Some(joint),
                message: None,
                body_codec: None,
            },
            None,
        )
        .await
    }

    async fn submit(
        &self,
        input: EvaluationInput,
        enqueued: Option<oneshot::Sender<()>>,
    ) -> ProxyResult<EvaluatedRules> {
        let epoch = input.context.runtime_epoch;
        let sender = self.sender_for(epoch)?;
        let (reply, response) = oneshot::channel();
        sender
            .send(Command::Evaluate {
                input: Box::new(input),
                reply: Reply::Async(reply),
            })
            .await
            .map_err(|_| unavailable(epoch))?;
        if let Some(enqueued) = enqueued {
            let _ = enqueued.send(());
        }
        response.await.map_err(|_| unavailable(epoch))?
    }

    fn sender_for(&self, epoch: Uuid) -> ProxyResult<RuleActorSender> {
        let mut actors = self.actors.lock();
        if !actors.active_epochs.contains(&epoch) {
            return Err(unavailable(epoch));
        }
        if let Some(sender) = actors.senders.get(&epoch) {
            return Ok(sender.clone());
        }
        let sender = actor::spawn(
            epoch,
            Arc::clone(&self.rules),
            Arc::clone(&self.events),
            Arc::clone(&self.channel_labels),
        );
        actors.senders.insert(epoch, sender.clone());
        Ok(sender)
    }

    pub(super) async fn runtime_stopping(&self, epoch: Uuid) {
        let sender = {
            let mut actors = self.actors.lock();
            actors.active_epochs.remove(&epoch);
            actors.senders.remove(&epoch)
        };
        let Some(sender) = sender else { return };
        let cleanup = tokio::spawn(async move {
            let (completed, completion) = oneshot::channel();
            if sender.send(Command::Stop { completed }).await.is_ok() {
                let _ = completion.await;
            }
        });
        let _ = cleanup.await;
    }

    pub(super) fn publish_rule_hits(&self, epoch: Uuid, rules: Vec<RuleSummaryViewModel>) {
        for rule in rules {
            self.events.publish(
                Some(epoch),
                Utc::now(),
                Some(rule.rule_id.to_string()),
                Some(rule.revision),
                UiEventPayload::RuleHit(rule),
            );
        }
    }
}

fn publish_repository_error(events: &EventHub, epoch: Uuid, error: &AppError) {
    events.publish(
        Some(epoch),
        Utc::now(),
        error.view_model.entity_id.clone(),
        None,
        UiEventPayload::OperationFailed((*error.view_model).clone()),
    );
}

fn unavailable(epoch: Uuid) -> ProxyError {
    ProxyError::new(
        ErrorCode::Internal,
        format!("rule runtime actor unavailable for epoch {epoch}"),
    )
}

fn matched_rule_summaries(
    evaluation: &intercept_proxy_domain::RuleEvaluation,
    rules: &[RuleDefinition],
    channel_labels: &BTreeMap<String, String>,
) -> Vec<RuleSummaryViewModel> {
    evaluation
        .traces
        .iter()
        .filter(|trace| trace.matched)
        .filter_map(|trace| {
            rules
                .iter()
                .find(|rule| rule.rule_id() == trace.rule_id)
                .map(|rule| rule_summary(rule, channel_labels))
        })
        .collect()
}

fn rule_trace_text(evaluation: &intercept_proxy_domain::RuleEvaluation) -> Vec<String> {
    evaluation
        .traces
        .iter()
        .map(|trace| {
            format!(
                "{} [{}] {}",
                trace.rule_id,
                if trace.matched { "命中" } else { "未命中" },
                trace.reason
            )
        })
        .collect()
}

fn rule_summary(
    rule: &RuleDefinition,
    channel_labels: &BTreeMap<String, String>,
) -> RuleSummaryViewModel {
    let (condition_count, action_count) = (1, 1);
    let listener = rule.listener_id().to_string();
    RuleSummaryViewModel {
        rule_id: rule.rule_id().as_uuid(),
        revision: rule.revision().get(),
        name: rule.name().to_owned(),
        enabled: rule.enabled(),
        priority: rule.priority(),
        creation_order: rule.created_order(),
        channel_text: channel_labels.get(&listener).cloned().unwrap_or(listener),
        stage_text: match rule.stage() {
            RuleStage::ProxyToUpstream => "请求",
            RuleStage::ProxyToApp => "响应",
        }
        .into(),
        match_summary: format!("{condition_count} 个条件"),
        action_summary: format!("{action_count} 个动作"),
        hit_count: rule.lifecycle().hit_count,
        last_hit_at: rule.lifecycle().last_hit_at,
        ui_tone: if rule.enabled() {
            UiTone::Positive
        } else {
            UiTone::Neutral
        },
    }
}
