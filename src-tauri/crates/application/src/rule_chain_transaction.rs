use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use intercept_proxy_domain::{
    Document, HttpAction, MatchField, MatchOperator, NthCounterAdvance, NthCounterSnapshot, RuleId,
    RuleLifecycleDelta, RuleLifecycleSnapshot, RuleProgramEntry, TerminalAction, TerminalIdentity,
    UnifiedAction, evaluate_conditions_with_nth,
};
use tokio_util::sync::CancellationToken;

use crate::{AppError, AppResult};

/// Application-owned HTTP working value. Infrastructure converts to and from runtime messages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingHttpMessage {
    pub target: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub status: Option<u16>,
}

impl WorkingHttpMessage {
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
            status: None,
        }
    }
}

/// Typed HTTP behavior used inside the private transaction checkpoint.
pub trait RuleChainHttpPort: std::fmt::Debug + Send + Sync {
    fn matches(
        &self,
        message: &WorkingHttpMessage,
        field: &MatchField,
        operator: &MatchOperator,
    ) -> AppResult<bool>;
    fn apply(&self, message: &mut WorkingHttpMessage, action: &HttpAction) -> AppResult<()>;
    fn encode_document(
        &self,
        document: &Document,
        message: &mut WorkingHttpMessage,
    ) -> AppResult<()>;
}

/// The only persistence boundary for Phase 6 lifecycle state.
#[async_trait]
pub trait RuleChainCommitPort: std::fmt::Debug + Send + Sync {
    async fn commit(&self, request: RuleChainCommitRequest) -> AppResult<u64>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleChainCommitRequest {
    pub expected_collection_revision: u64,
    pub deltas: Vec<RuleLifecycleDelta>,
}

#[derive(Clone, Debug)]
pub struct RuleChainInput {
    pub expected_collection_revision: u64,
    pub message: WorkingHttpMessage,
    pub document: Document,
    pub terminal: TerminalIdentity,
    pub plan: RuleChainPlan,
    pub evaluated_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct RuleChainPlanEntry {
    program: RuleProgramEntry,
    lifecycle: RuleLifecycleSnapshot,
    nth_counter: NthCounterSnapshot,
}

impl RuleChainPlanEntry {
    pub fn new(
        program: RuleProgramEntry,
        lifecycle: RuleLifecycleSnapshot,
        nth_counter: NthCounterSnapshot,
    ) -> AppResult<Self> {
        let rule_id = program.rule_id();
        if lifecycle.rule_id != rule_id || nth_counter.rule_id != rule_id {
            return Err(AppError::new(
                "RULE_INVALID",
                "规则计划的 program、lifecycle 与 Nth owner 必须一致。",
            ));
        }
        Ok(Self {
            program,
            lifecycle,
            nth_counter,
        })
    }
}

#[derive(Clone, Debug)]
pub struct RuleChainPlan {
    entries: Vec<RuleChainPlanEntry>,
}

impl RuleChainPlan {
    pub fn new(mut entries: Vec<RuleChainPlanEntry>) -> AppResult<Self> {
        let mut ids = BTreeSet::new();
        if entries
            .iter()
            .any(|entry| !ids.insert(entry.program.rule_id()))
        {
            return Err(AppError::new(
                "RULE_INVALID",
                "规则计划不得包含重复 rule_id。",
            ));
        }
        entries.sort_by_key(|entry| (entry.program.priority(), entry.program.rule_id()));
        Ok(Self { entries })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleChainOutput {
    pub collection_revision: u64,
    pub message: WorkingHttpMessage,
    pub document: Document,
    pub matched_rule_ids: Vec<RuleId>,
    pub terminal_action: Option<TerminalAction>,
    pub trace: Vec<RuleChainTrace>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleChainTrace {
    pub rule_id: RuleId,
    pub matched: bool,
}

/// Single owner of working HTTP/Document state and lifecycle deltas.
#[derive(Debug)]
pub struct RuleChainTransaction {
    http: Arc<dyn RuleChainHttpPort>,
    commit: Arc<dyn RuleChainCommitPort>,
}

impl RuleChainTransaction {
    #[must_use]
    pub fn new(http: Arc<dyn RuleChainHttpPort>, commit: Arc<dyn RuleChainCommitPort>) -> Self {
        Self { http, commit }
    }

    pub async fn execute(&self, input: RuleChainInput) -> AppResult<RuleChainOutput> {
        self.execute_cancellable(input, &CancellationToken::new())
            .await
    }

    pub async fn execute_cancellable(
        &self,
        input: RuleChainInput,
        cancellation: &CancellationToken,
    ) -> AppResult<RuleChainOutput> {
        validate_terminal_identity(&input)?;
        let mut working_message = input.message;
        let mut working_document = input.document;
        let mut matched_rule_ids = Vec::new();
        let mut deltas = Vec::new();
        let mut terminal_action = None;
        let mut trace = Vec::new();

        for planned in input.plan.entries {
            let entry = planned.program;
            let lifecycle = planned.lifecycle;
            cancelled(cancellation)?;
            if !lifecycle.enabled {
                continue;
            }
            let nth_attempt = planned.nth_counter.attempts.checked_add(1).ok_or_else(|| {
                AppError::new("RULE_INVALID", "Nth counter 已溢出。")
                    .entity(entry.rule_id().to_string())
            })?;
            let evaluation = evaluate_conditions_with_nth(
                entry.conditions(),
                &working_document,
                nth_attempt,
                &mut |field, operator| self.http.matches(&working_message, field, operator),
            )?;
            let matched = evaluation.matched;
            let nth_counter_advance = (evaluation.contains_nth && evaluation.eligible_without_nth)
                .then(|| NthCounterAdvance {
                    rule_id: entry.rule_id(),
                    terminal: input.terminal.clone(),
                    expected_attempts: planned.nth_counter.attempts,
                    increment: 1,
                });
            trace.push(RuleChainTrace {
                rule_id: entry.rule_id(),
                matched,
            });
            if !matched {
                if let Some(nth_counter_advance) = nth_counter_advance {
                    deltas.push(RuleLifecycleDelta {
                        rule_id: entry.rule_id(),
                        expected_revision: lifecycle.revision,
                        hit_count_increment: 0,
                        last_hit_at: None,
                        disable_one_shot: false,
                        nth_counter_advance: Some(nth_counter_advance),
                    });
                }
                continue;
            }
            for action in entry.actions() {
                cancelled(cancellation)?;
                match action {
                    UnifiedAction::Document(mutation) => mutation
                        .apply(&mut working_document)
                        .map_err(AppError::from)?,
                    UnifiedAction::Http(action) => self.http.apply(&mut working_message, action)?,
                    UnifiedAction::RecordMatch => {}
                    UnifiedAction::Terminal(action) => terminal_action = Some(action.clone()),
                }
            }
            matched_rule_ids.push(entry.rule_id());
            let mut delta = lifecycle.delta_for_successful_match(input.evaluated_at);
            delta.nth_counter_advance = nth_counter_advance;
            deltas.push(delta);
            if terminal_action.is_some() {
                break;
            }
        }

        cancelled(cancellation)?;
        self.http
            .encode_document(&working_document, &mut working_message)?;
        cancelled(cancellation)?;
        let collection_revision = if deltas.is_empty() {
            input.expected_collection_revision
        } else {
            self.commit
                .commit(RuleChainCommitRequest {
                    expected_collection_revision: input.expected_collection_revision,
                    deltas,
                })
                .await?
        };
        Ok(RuleChainOutput {
            collection_revision,
            message: working_message,
            document: working_document,
            matched_rule_ids,
            terminal_action,
            trace,
        })
    }
}

fn cancelled(cancellation: &CancellationToken) -> AppResult<()> {
    if cancellation.is_cancelled() {
        return Err(AppError::new(
            "RULE_EXECUTION_CANCELLED",
            "规则事务已取消。",
        ));
    }
    Ok(())
}

fn validate_terminal_identity(input: &RuleChainInput) -> AppResult<()> {
    if input
        .plan
        .entries
        .iter()
        .any(|entry| entry.nth_counter.terminal != input.terminal)
    {
        return Err(AppError::new(
            "RULE_INVALID",
            "Nth counter 的终端身份与事务输入不一致。",
        ));
    }
    Ok(())
}
