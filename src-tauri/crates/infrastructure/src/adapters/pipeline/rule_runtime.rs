//! Epoch-owned rule actors serialize evaluation and durable metadata commits.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, mpsc as std_mpsc},
};

use chrono::Utc;
use intercept_proxy_application::{
    AppError, EventHub, RuleSummaryViewModel, UiEventPayload, UiTone,
};
use intercept_proxy_domain::{MessageStage, Rule, RuleAction};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::{
    ConnectionContext, ErrorCode, Message, ProxyError, Result as ProxyResult,
    TLS_HANDSHAKE_POLICY_TIMEOUT,
};
use parking_lot::Mutex;
use tokio::sync::oneshot;
use uuid::Uuid;

mod actor;

use actor::{Command, EvaluationInput, Reply, RuleActorSender};

use super::{
    JointDocumentEvaluation, RuntimeRuleRepository,
    message_projection::{decode_json, message_target},
};

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
    pub(super) actions: Vec<RuleAction>,
    pub(super) traces: Vec<String>,
    pub(super) matched_ids: Vec<Uuid>,
    pub(super) hit_rules: Vec<RuleSummaryViewModel>,
    pub(super) prepared_message: Option<Message>,
    pub(super) prepared_socket: Option<intercept_proxy_exchange::SocketContext>,
    pub(super) fault_actions: Vec<intercept_proxy_runtime::FaultAction>,
    pub(super) pause: bool,
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
        message: Option<&Message>,
        body_codec: Arc<dyn BodyCodec>,
        joint_document: Option<JointDocumentEvaluation>,
    ) -> ProxyResult<EvaluatedRules> {
        let input = EvaluationInput {
            context: context.clone(),
            stage,
            json: message.and_then(|message| decode_json(body_codec.as_ref(), &message.body).ok()),
            target: message
                .and_then(|message| message_target(&message.start_line).map(str::to_owned)),
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
                json: None,
                target: None,
                joint_document: None,
                socket_joint: Some(joint),
                message: None,
                body_codec: None,
            },
            None,
        )
        .await
    }

    #[cfg(test)]
    pub(super) async fn evaluate_with_enqueue_notification(
        &self,
        context: &ConnectionContext,
        stage: MessageStage,
        message: Option<&Message>,
        body_codec: &dyn BodyCodec,
        enqueued: oneshot::Sender<()>,
    ) -> ProxyResult<EvaluatedRules> {
        let input = EvaluationInput {
            context: context.clone(),
            stage,
            json: message.and_then(|message| decode_json(body_codec, &message.body).ok()),
            target: message
                .and_then(|message| message_target(&message.start_line).map(str::to_owned)),
            joint_document: None,
            socket_joint: None,
            message: None,
            body_codec: None,
        };
        self.submit(input, Some(enqueued)).await
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

    pub(super) fn evaluate_handshake(
        &self,
        context: &ConnectionContext,
    ) -> ProxyResult<EvaluatedRules> {
        let sender = self.existing_sender(context.runtime_epoch)?;
        let (reply, response) = std_mpsc::sync_channel(1);
        sender
            .try_send(Command::Evaluate {
                input: Box::new(EvaluationInput {
                    context: context.clone(),
                    stage: MessageStage::TlsHandshake,
                    json: None,
                    target: None,
                    joint_document: None,
                    socket_joint: None,
                    message: None,
                    body_codec: None,
                }),
                reply: Reply::Handshake(reply),
            })
            .map_err(|error| {
                self.publish_failure(
                    context.runtime_epoch,
                    "TLS_RULE_ACTOR_UNAVAILABLE",
                    format!("TLS 规则 actor 无法接收握手策略请求：{error}"),
                );
                unavailable(context.runtime_epoch)
            })?;
        response
            .recv_timeout(TLS_HANDSHAKE_POLICY_TIMEOUT)
            .map_err(|error| {
                self.publish_failure(
                    context.runtime_epoch,
                    "TLS_RULE_POLICY_TIMEOUT",
                    format!("TLS 握手规则策略未在限定时间内完成：{error}"),
                );
                ProxyError::new(
                    ErrorCode::TlsHandshakeFailed,
                    "TLS handshake rule policy timed out",
                )
            })?
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

    fn existing_sender(&self, epoch: Uuid) -> ProxyResult<RuleActorSender> {
        self.actors
            .lock()
            .senders
            .get(&epoch)
            .cloned()
            .ok_or_else(|| unavailable(epoch))
    }

    fn publish_failure(&self, epoch: Uuid, code: &'static str, message: String) {
        let error = AppError::new(code, message);
        self.events.publish(
            Some(epoch),
            Utc::now(),
            None,
            None,
            UiEventPayload::OperationFailed((*error.view_model).clone()),
        );
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

    #[cfg(test)]
    pub(super) fn registry_counts(&self) -> (usize, usize) {
        let actors = self.actors.lock();
        (actors.active_epochs.len(), actors.senders.len())
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
    rules: &[Rule],
    channel_labels: &BTreeMap<String, String>,
) -> Vec<RuleSummaryViewModel> {
    evaluation
        .traces
        .iter()
        .filter(|trace| trace.matched)
        .filter_map(|trace| {
            rules
                .iter()
                .find(|rule| rule.id == trace.rule_id)
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

fn rule_summary(rule: &Rule, channel_labels: &BTreeMap<String, String>) -> RuleSummaryViewModel {
    RuleSummaryViewModel {
        rule_id: rule.id.as_uuid(),
        revision: rule.revision.get(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        priority: i32::try_from(rule.priority).unwrap_or(i32::MAX),
        creation_order: rule.created_order,
        channel_text: rule.channel.as_ref().map_or_else(
            || "全部".into(),
            |channel| {
                channel_labels
                    .get(channel.as_str())
                    .cloned()
                    .unwrap_or_else(|| channel.to_string())
            },
        ),
        stage_text: match rule.stage {
            MessageStage::Request => "请求",
            MessageStage::Response => "响应",
            MessageStage::TlsHandshake => "TLS 握手",
        }
        .into(),
        match_summary: format!("{} 个条件", rule.conditions.len()),
        action_summary: format!("{} 个动作", rule.actions.len()),
        hit_count: rule.hit_count,
        last_hit_at: rule.last_hit_at,
        ui_tone: if rule.enabled {
            UiTone::Positive
        } else {
            UiTone::Neutral
        },
    }
}
