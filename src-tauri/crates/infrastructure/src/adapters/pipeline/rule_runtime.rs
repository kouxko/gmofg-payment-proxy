//! Serialized rule evaluation and durable runtime metadata coordination.

use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;
use gmofg_proxy_application::{EventHub, RuleSummaryViewModel, UiEventPayload, UiTone};
use gmofg_proxy_domain::{
    MatchContext, MessageStage, Rule, RuleAction, RuleEngine, RuleRuntimeSnapshot, RuntimeEpoch,
};
use gmofg_proxy_product_api::BodyCodec;
use gmofg_proxy_runtime::{ConnectionContext, Message, Result as ProxyResult};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{
    RuntimeRuleRepository, app_to_proxy, domain_channel,
    message_projection::{decode_json, message_target},
    rule_actions::terminal_identity,
};

#[derive(Debug)]
pub(super) struct RuleRuntimeService {
    body_codec: Arc<dyn BodyCodec>,
    channel_labels: BTreeMap<String, String>,
    rules: Arc<dyn RuntimeRuleRepository>,
    events: Arc<EventHub>,
    epoch: Mutex<Option<Uuid>>,
    runtime: Mutex<Option<RuleRuntime>>,
}

#[derive(Debug)]
struct RuleRuntime {
    epoch: Uuid,
    snapshot: RuleRuntimeSnapshot,
    engine: RuleEngine,
}

#[derive(Debug)]
pub(super) struct EvaluatedRules {
    pub(super) actions: Vec<RuleAction>,
    pub(super) traces: Vec<String>,
    pub(super) matched_ids: Vec<Uuid>,
    pub(super) hit_rules: Vec<RuleSummaryViewModel>,
}

impl RuleRuntimeService {
    pub(super) fn new(
        body_codec: Arc<dyn BodyCodec>,
        channel_labels: BTreeMap<String, String>,
        rules: Arc<dyn RuntimeRuleRepository>,
        events: Arc<EventHub>,
    ) -> Self {
        Self {
            body_codec,
            channel_labels,
            rules,
            events,
            epoch: Mutex::new(None),
            runtime: Mutex::new(None),
        }
    }

    pub(super) fn evaluate(
        &self,
        context: &ConnectionContext,
        stage: MessageStage,
        message: Option<&Message>,
    ) -> ProxyResult<EvaluatedRules> {
        self.evaluate_with_retries(context, stage, message, 3)
    }

    fn evaluate_with_retries(
        &self,
        context: &ConnectionContext,
        stage: MessageStage,
        message: Option<&Message>,
        remaining_retries: usize,
    ) -> ProxyResult<EvaluatedRules> {
        self.ensure_epoch(context.runtime_epoch)?;
        let terminal = terminal_identity(context);
        let json =
            message.and_then(|message| decode_json(self.body_codec.as_ref(), &message.body).ok());
        let target = message.and_then(|message| message_target(&message.start_line));
        let runtime_epoch = RuntimeEpoch::from_uuid(context.runtime_epoch);

        // Evaluation and its durable runtime metadata commit are one serialized
        // operation. Actions are never returned to the transport until the
        // corresponding hit count / one-shot disable transaction commits.
        let mut runtime_state = self.runtime.lock();
        let snapshot = self.rules.runtime_snapshot().map_err(app_to_proxy)?;
        if runtime_state
            .as_ref()
            .is_none_or(|runtime| runtime.epoch != context.runtime_epoch)
        {
            *runtime_state = Some(RuleRuntime {
                epoch: context.runtime_epoch,
                engine: RuleEngine::new(runtime_epoch, snapshot.rules.clone()),
                snapshot,
            });
        } else if let Some(runtime) = runtime_state.as_mut()
            && (runtime.snapshot.collection_revision != snapshot.collection_revision
                || runtime.snapshot.signature != snapshot.signature)
        {
            runtime.engine.reconcile(snapshot.rules.clone());
            runtime.snapshot = snapshot;
        }
        let runtime = runtime_state
            .as_mut()
            .expect("rule runtime was initialized");

        // A failed durable commit must not consume this message's transient
        // NthHit increments. Restore this checkpoint before re-evaluating
        // against the newer persisted collection snapshot.
        let engine_before_evaluation = runtime.engine.clone();
        let evaluation = runtime.engine.evaluate(
            &MatchContext {
                runtime_epoch,
                channel: domain_channel(&context.channel)?,
                stage,
                terminal: &terminal,
                path_or_request_type: target,
                json_body: json.as_ref(),
            },
            Utc::now(),
        );
        let hit_rules =
            matched_rule_summaries(&evaluation, runtime.engine.rules(), &self.channel_labels);
        let matched = evaluation.traces.iter().any(|trace| trace.matched);
        if matched {
            let base_snapshot = runtime.snapshot.clone();
            let evaluated_rules = runtime.engine.rules().to_vec();
            let next_collection_revision = match self
                .rules
                .commit_runtime_snapshot(&base_snapshot, &evaluated_rules)
            {
                Ok(revision) => revision,
                Err(error)
                    if error.view_model.code == "REVISION_CONFLICT" && remaining_retries > 0 =>
                {
                    runtime.engine = engine_before_evaluation;
                    drop(runtime_state);
                    return self.evaluate_with_retries(
                        context,
                        stage,
                        message,
                        remaining_retries - 1,
                    );
                }
                Err(error) => {
                    *runtime_state = None;
                    drop(runtime_state);
                    self.events.publish(
                        Some(context.runtime_epoch),
                        Utc::now(),
                        error.view_model.entity_id.clone(),
                        None,
                        UiEventPayload::OperationFailed((*error.view_model).clone()),
                    );
                    return Err(app_to_proxy(error));
                }
            };
            runtime.snapshot = RuleRuntimeSnapshot::with_collection_revision(
                next_collection_revision,
                evaluated_rules,
            );
        }

        let traces = rule_trace_text(&evaluation);
        let matched_ids = evaluation
            .traces
            .iter()
            .filter(|trace| trace.matched)
            .map(|trace| trace.rule_id.as_uuid())
            .collect();
        drop(runtime_state);
        Ok(EvaluatedRules {
            actions: evaluation.composed_actions,
            traces,
            matched_ids,
            hit_rules,
        })
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

    pub(super) fn runtime_stopping(&self, epoch: Uuid) {
        if let Err(error) = self.rules.reset_runtime_hit_metadata() {
            self.events.publish(
                Some(epoch),
                Utc::now(),
                error.view_model.entity_id.clone(),
                None,
                UiEventPayload::OperationFailed((*error.view_model).clone()),
            );
        }
        *self.runtime.lock() = None;
        *self.epoch.lock() = None;
    }

    fn ensure_epoch(&self, epoch: Uuid) -> ProxyResult<()> {
        let mut current = self.epoch.lock();
        if *current != Some(epoch) {
            self.rules
                .reset_runtime_hit_metadata()
                .map_err(app_to_proxy)?;
            *self.runtime.lock() = None;
            *current = Some(epoch);
        }
        Ok(())
    }
}

fn matched_rule_summaries(
    evaluation: &gmofg_proxy_domain::RuleEvaluation,
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

fn rule_trace_text(evaluation: &gmofg_proxy_domain::RuleEvaluation) -> Vec<String> {
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
