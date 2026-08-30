use std::{collections::HashMap, convert::Infallible};

use chrono::{DateTime, Utc};

use crate::{
    DomainError, ErrorCode, NthCounterSnapshot, Revision, RuleId, RuntimeEpoch, TerminalIdentity,
};

use super::{
    HttpAction, MatchContext, Rule, RuleConflictWarning, RuleDraft, RuleEvaluation, RuleTrace,
    matching::matches_condition, validate_rule_draft,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CounterKey {
    rule_id: RuleId,
    source_ip: String,
    certificate_sha256: String,
}

#[derive(Clone, Debug, Default)]
pub struct RuleEngine {
    pub(super) runtime_epoch: Option<RuntimeEpoch>,
    rules: Vec<Rule>,
    counters: HashMap<CounterKey, u64>,
}

impl RuleEngine {
    #[must_use]
    pub fn new(runtime_epoch: RuntimeEpoch, rules: Vec<Rule>) -> Self {
        Self {
            runtime_epoch: Some(runtime_epoch),
            rules,
            counters: HashMap::new(),
        }
    }

    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    #[must_use]
    pub fn nth_counter_snapshots(&self) -> Vec<NthCounterSnapshot> {
        self.counters
            .iter()
            .map(|(key, attempts)| NthCounterSnapshot {
                rule_id: key.rule_id,
                terminal: TerminalIdentity {
                    source_ip: key.source_ip.clone(),
                    certificate_sha256: key.certificate_sha256.clone(),
                },
                attempts: *attempts,
            })
            .collect()
    }

    pub fn restart(&mut self, runtime_epoch: RuntimeEpoch) {
        self.runtime_epoch = Some(runtime_epoch);
        self.counters.clear();
        for rule in &mut self.rules {
            rule.hit_count = 0;
            rule.last_hit_at = None;
        }
    }

    pub fn reconcile(&mut self, rules: Vec<Rule>) {
        let reset_ids = rules
            .iter()
            .filter_map(|next| {
                let previous = self.rules.iter().find(|rule| rule.id == next.id);
                match previous {
                    Some(previous)
                        if previous.conditions == next.conditions
                            && (previous.enabled || !next.enabled) =>
                    {
                        None
                    }
                    _ => Some(next.id),
                }
            })
            .collect::<Vec<_>>();
        self.counters.retain(|key, _| {
            rules.iter().any(|rule| rule.id == key.rule_id) && !reset_ids.contains(&key.rule_id)
        });
        self.rules = rules;
    }

    pub fn save(&mut self, id: RuleId, draft: RuleDraft) -> Result<Revision, DomainError> {
        validate_rule_draft(&draft)?;
        let (must_reset, revision) = {
            let rule = self
                .rules
                .iter_mut()
                .find(|rule| rule.id == id)
                .ok_or_else(|| DomainError::new(ErrorCode::RuleInvalid, "规则不存在"))?;
            let expected = draft.expected_revision.ok_or_else(|| {
                DomainError::new(ErrorCode::RevisionConflict, "修改规则必须提供当前 revision")
            })?;
            rule.revision.verify(expected)?;
            let must_reset =
                rule.conditions != draft.conditions || (!rule.enabled && draft.enabled);
            rule.apply_draft(draft);
            (must_reset, rule.revision)
        };
        if must_reset {
            self.reset_rule_hits(id);
        }
        Ok(revision)
    }

    pub fn toggle(
        &mut self,
        id: RuleId,
        expected_revision: Revision,
        enabled: bool,
    ) -> Result<Revision, DomainError> {
        let rule = self
            .rules
            .iter_mut()
            .find(|rule| rule.id == id)
            .ok_or_else(|| DomainError::new(ErrorCode::RuleInvalid, "规则不存在"))?;
        rule.revision.verify(expected_revision)?;
        let reset = !rule.enabled && enabled;
        rule.enabled = enabled;
        rule.revision = rule.revision.next();
        let revision = rule.revision;
        if reset {
            self.reset_rule_hits(id);
        }
        Ok(revision)
    }

    pub fn evaluate(&mut self, context: &MatchContext<'_>, now: DateTime<Utc>) -> RuleEvaluation {
        self.evaluate_with_gate(context, now, |_| Ok::<_, Infallible>(true))
            .expect("an infallible rule gate cannot fail")
    }

    /// Evaluates the ordinary HTTP conditions and an additional per-rule gate as one atomic match.
    ///
    /// The gate runs only after all ordinary conditions match. A false gate does not consume
    /// `NthHit`, increment hit metadata, or disable a one-shot rule. If the gate fails, no rule
    /// metadata is committed; callers can therefore discard any private working value mutated by
    /// the gate and preserve all-or-nothing execution across typed Document and HTTP actions.
    pub fn evaluate_with_gate<E>(
        &mut self,
        context: &MatchContext<'_>,
        now: DateTime<Utc>,
        gate: impl FnMut(&Rule) -> Result<bool, E>,
    ) -> Result<RuleEvaluation, E> {
        self.evaluate_with_gate_in_order(context, now, &[], gate)
    }

    pub fn evaluate_with_gate_in_order<E>(
        &mut self,
        context: &MatchContext<'_>,
        now: DateTime<Utc>,
        execution_order: &[RuleId],
        mut gate: impl FnMut(&Rule) -> Result<bool, E>,
    ) -> Result<RuleEvaluation, E> {
        if self.runtime_epoch != Some(context.runtime_epoch) {
            self.restart(context.runtime_epoch);
        }
        let mut snapshot = self.rules.clone();
        let order = execution_order
            .iter()
            .enumerate()
            .map(|(index, rule_id)| (*rule_id, index))
            .collect::<HashMap<_, _>>();
        snapshot.sort_by_key(|rule| {
            (
                order.get(&rule.id).copied().unwrap_or(usize::MAX),
                rule.priority,
                rule.id,
            )
        });
        let mut evaluation = RuleEvaluation::default();
        let mut hit_ids = Vec::new();
        let counters_before_evaluation = self.counters.clone();

        for rule in snapshot.iter().filter(|rule| rule.enabled) {
            if rule
                .channel
                .as_ref()
                .is_some_and(|channel| channel != &context.channel)
                || rule.stage != context.stage
            {
                continue;
            }
            let counters_before_rule = self.counters.clone();
            match self.matches_rule(rule, context) {
                Ok(true) => match gate(rule) {
                    Ok(true) => {
                        hit_ids.push(rule.id);
                        let mut executed = Vec::new();
                        for action in &rule.actions {
                            executed.push(action.clone());
                            evaluation.composed_actions.push(action.clone());
                            if let HttpAction::Terminal(terminal) = action {
                                evaluation.terminal_action = Some(terminal.clone());
                                break;
                            }
                        }
                        evaluation.traces.push(RuleTrace {
                            rule_id: rule.id,
                            matched: true,
                            reason: "全部匹配条件满足".into(),
                            actions: executed,
                        });
                        if evaluation.terminal_action.is_some() {
                            break;
                        }
                    }
                    Ok(false) => {
                        self.counters = counters_before_rule;
                        evaluation.traces.push(RuleTrace {
                            rule_id: rule.id,
                            matched: false,
                            reason: "扩展匹配条件不满足".into(),
                            actions: Vec::new(),
                        });
                    }
                    Err(error) => {
                        self.counters = counters_before_evaluation;
                        return Err(error);
                    }
                },
                Ok(false) => evaluation.traces.push(RuleTrace {
                    rule_id: rule.id,
                    matched: false,
                    reason: "匹配条件不满足".into(),
                    actions: Vec::new(),
                }),
                Err(reason) => evaluation.traces.push(RuleTrace {
                    rule_id: rule.id,
                    matched: false,
                    reason,
                    actions: Vec::new(),
                }),
            }
        }
        self.commit_hits(hit_ids, now);
        Ok(evaluation)
    }

    #[must_use]
    pub fn conflict_warnings(&self) -> Vec<RuleConflictWarning> {
        let mut sorted: Vec<&Rule> = self.rules.iter().filter(|rule| rule.enabled).collect();
        sorted.sort_by_key(|rule| (rule.priority, rule.id));
        let mut warnings = Vec::new();
        for (index, higher) in sorted.iter().enumerate() {
            if !higher.actions.iter().any(HttpAction::is_terminal) {
                continue;
            }
            for lower in sorted.iter().skip(index + 1) {
                if higher.stage == lower.stage
                    && (higher.channel.is_none() || higher.channel == lower.channel)
                    && higher
                        .conditions
                        .iter()
                        .all(|condition| lower.conditions.contains(condition))
                {
                    warnings.push(RuleConflictWarning {
                        code: ErrorCode::RuleConflictWarning,
                        shadowing_rule_id: higher.id,
                        shadowed_rule_id: lower.id,
                        message: format!("规则“{}”可能遮蔽规则“{}”", higher.name, lower.name),
                    });
                }
            }
        }
        warnings
    }

    fn commit_hits(&mut self, hit_ids: Vec<RuleId>, now: DateTime<Utc>) {
        for id in hit_ids {
            if let Some(rule) = self.rules.iter_mut().find(|rule| rule.id == id) {
                rule.hit_count = rule.hit_count.saturating_add(1);
                rule.last_hit_at = Some(now);
                if rule.one_shot {
                    rule.enabled = false;
                    rule.revision = rule.revision.next();
                }
            }
        }
    }

    fn reset_rule_hits(&mut self, id: RuleId) {
        self.counters.retain(|key, _| key.rule_id != id);
        if let Some(rule) = self.rules.iter_mut().find(|rule| rule.id == id) {
            rule.hit_count = 0;
            rule.last_hit_at = None;
        }
    }

    fn matches_rule(&mut self, rule: &Rule, context: &MatchContext<'_>) -> Result<bool, String> {
        for condition in rule
            .conditions
            .iter()
            .filter(|condition| !matches!(condition, crate::Condition::NthHit { .. }))
        {
            let crate::Condition::Http { field, operator } = condition else {
                return Err("旧 HTTP runtime 不支持 Document 条件".into());
            };
            if !matches_condition(field, operator, context)? {
                return Ok(false);
            }
        }
        let nth_values = rule
            .conditions
            .iter()
            .filter_map(|condition| match condition {
                crate::Condition::NthHit { count } => Some(*count),
                crate::Condition::Http { .. } | crate::Condition::Document { .. } => None,
            })
            .collect::<Vec<_>>();
        if nth_values.is_empty() {
            return Ok(true);
        }
        let key = CounterKey {
            rule_id: rule.id,
            source_ip: context.terminal.source_ip.clone(),
            certificate_sha256: context.terminal.certificate_sha256.clone(),
        };
        let count = self.counters.entry(key).or_default();
        *count = count.saturating_add(1);
        Ok(nth_values.iter().all(|nth| *nth == *count))
    }
}
