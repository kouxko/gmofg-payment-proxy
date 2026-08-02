//! 串行化规则求值，并协调内存快照与持久化运行元数据。
//!
//! 单个互斥操作保证“读取快照、匹配、增加命中信息、CAS 持久化”的次序可解释；revision
//! 冲突会重新加载或显式失败，不会覆盖用户刚编辑的规则。持久化错误与消息动作结果分层
//! 处理，避免数据库故障导致已处理网络消息被重复执行。

use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;
use intercept_proxy_application::{EventHub, RuleSummaryViewModel, UiEventPayload, UiTone};
use intercept_proxy_domain::{
    MatchContext, MessageStage, Rule, RuleAction, RuleEngine, RuleRuntimeSnapshot, RuntimeEpoch,
};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::{ConnectionContext, Message, Result as ProxyResult};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{
    RuntimeRuleRepository, app_to_proxy, domain_channel,
    message_projection::{decode_json, message_target},
    rule_actions::terminal_identity,
};

#[derive(Debug)]
pub(super) struct RuleRuntimeService {
    channel_labels: BTreeMap<String, String>,
    rules: Arc<dyn RuntimeRuleRepository>,
    events: Arc<EventHub>,
    runtimes: Mutex<BTreeMap<Uuid, RuleRuntime>>,
}

#[derive(Debug)]
struct RuleRuntime {
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
        channel_labels: BTreeMap<String, String>,
        rules: Arc<dyn RuntimeRuleRepository>,
        events: Arc<EventHub>,
    ) -> Self {
        Self {
            channel_labels,
            rules,
            events,
            runtimes: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) fn evaluate(
        &self,
        context: &ConnectionContext,
        stage: MessageStage,
        message: Option<&Message>,
        body_codec: &dyn BodyCodec,
    ) -> ProxyResult<EvaluatedRules> {
        // 重试只处理 CAS 冲突：每次都恢复引擎检查点并重新读取持久化快照，不能直接重放
        // 上一次 actions，否则 NthHit/一次性规则可能被消费两次。
        self.evaluate_with_retries(context, stage, message, body_codec, 3)
    }

    fn evaluate_with_retries(
        &self,
        context: &ConnectionContext,
        stage: MessageStage,
        message: Option<&Message>,
        body_codec: &dyn BodyCodec,
        remaining_retries: usize,
    ) -> ProxyResult<EvaluatedRules> {
        let terminal = terminal_identity(context);
        let json = message.and_then(|message| decode_json(body_codec, &message.body).ok());
        let target = message.and_then(|message| message_target(&message.start_line));
        let runtime_epoch = RuntimeEpoch::from_uuid(context.runtime_epoch);

        // Evaluation and its durable runtime metadata commit are one serialized
        // operation. Actions are never returned to the transport until the
        // corresponding hit count / one-shot disable transaction commits.
        let mut runtime_state = self.runtimes.lock();
        self.prepare_runtime(&mut runtime_state, context, runtime_epoch)?;
        let runtime = runtime_state
            .get_mut(&context.runtime_epoch)
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
                        body_codec,
                        remaining_retries - 1,
                    );
                }
                Err(error) => {
                    runtime_state.remove(&context.runtime_epoch);
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
            runtime.snapshot = RuleRuntimeSnapshot::with_collection_identity(
                base_snapshot.collection_id,
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

    fn prepare_runtime(
        &self,
        runtimes: &mut BTreeMap<Uuid, RuleRuntime>,
        context: &ConnectionContext,
        runtime_epoch: RuntimeEpoch,
    ) -> ProxyResult<()> {
        let mut snapshot = self
            .rules
            .runtime_snapshot(&context.channel)
            .map_err(app_to_proxy)?;
        match runtimes.entry(context.runtime_epoch) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                if let Some(collection_id) = snapshot.collection_id {
                    self.rules
                        .reset_runtime_hit_metadata(collection_id)
                        .map_err(app_to_proxy)?;
                    snapshot = self
                        .rules
                        .runtime_snapshot(&context.channel)
                        .map_err(app_to_proxy)?;
                }
                entry.insert(RuleRuntime {
                    engine: RuleEngine::new(runtime_epoch, snapshot.rules.clone()),
                    snapshot,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let runtime = entry.get_mut();
                if runtime.snapshot.collection_id != snapshot.collection_id
                    || runtime.snapshot.collection_revision != snapshot.collection_revision
                    || runtime.snapshot.signature != snapshot.signature
                {
                    runtime.engine.reconcile(snapshot.rules.clone());
                    runtime.snapshot = snapshot;
                }
            }
        }
        Ok(())
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
        let runtime = self.runtimes.lock().remove(&epoch);
        if let Some(collection_id) = runtime.and_then(|item| item.snapshot.collection_id)
            && let Err(error) = self.rules.reset_runtime_hit_metadata(collection_id)
        {
            self.events.publish(
                Some(epoch),
                Utc::now(),
                error.view_model.entity_id.clone(),
                None,
                UiEventPayload::OperationFailed((*error.view_model).clone()),
            );
        }
    }
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
