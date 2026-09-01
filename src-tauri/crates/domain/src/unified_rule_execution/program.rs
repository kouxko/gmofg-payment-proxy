use std::collections::BTreeSet;

use super::{
    Condition, ConditionEvaluation, Document, DomainError, HttpAction, MatchField, MatchOperator,
    RuleId, TerminalAction, UnifiedAction, evaluate_condition, rule_error,
};

/// Immutable rule program input. `created_order` is history/UI metadata only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleProgramEntry {
    rule_id: RuleId,
    priority: i32,
    created_order: u64,
    condition: Condition,
    action: UnifiedAction,
}

impl RuleProgramEntry {
    /// Constructs a validated, independent rule program entry.
    pub fn new(
        rule_id: RuleId,
        priority: i32,
        created_order: u64,
        condition: Condition,
        action: UnifiedAction,
    ) -> Result<Self, DomainError> {
        validate_action(&action)?;
        Ok(Self {
            rule_id,
            priority,
            created_order,
            condition,
            action,
        })
    }

    #[must_use]
    pub const fn rule_id(&self) -> RuleId {
        self.rule_id
    }

    #[must_use]
    pub const fn priority(&self) -> i32 {
        self.priority
    }

    #[must_use]
    pub const fn created_order(&self) -> u64 {
        self.created_order
    }

    #[must_use]
    pub const fn condition(&self) -> &Condition {
        &self.condition
    }

    #[must_use]
    pub const fn action(&self) -> &UnifiedAction {
        &self.action
    }

    pub fn replace_condition(&mut self, condition: Condition) {
        self.condition = condition;
    }
}

fn validate_action(action: &UnifiedAction) -> Result<(), DomainError> {
    if matches!(action, UnifiedAction::Http(HttpAction::Terminal(_))) {
        return Err(rule_error(
            "action",
            "终止动作必须使用统一 terminal 变体",
        ));
    }
    Ok(())
}

/// Deterministic, immutable Phase 5 rule program.
#[derive(Clone, Debug)]
pub struct UnifiedRuleProgram {
    rules: Vec<RuleProgramEntry>,
}

impl UnifiedRuleProgram {
    /// Validates and sorts rules exclusively by `(priority, rule_id)`.
    pub fn new(mut rules: Vec<RuleProgramEntry>) -> Result<Self, DomainError> {
        let mut rule_ids = BTreeSet::new();
        for rule in &rules {
            if !rule_ids.insert(rule.rule_id) {
                return Err(rule_error("rules.rule_id", "规则 ID 不能重复"));
            }
            validate_action(&rule.action)?;
        }
        rules.sort_by_key(|rule| (rule.priority, rule.rule_id));
        Ok(Self { rules })
    }

    pub fn execute(&self, document: Document) -> Result<UnifiedRuleExecution, DomainError> {
        self.execute_with_http(document, |_, _| {
            Err(rule_error(
                "conditions",
                "HTTP 条件需要应用层提供类型化 HTTP 上下文",
            ))
        })
    }

    #[must_use]
    pub fn rules(&self) -> &[RuleProgramEntry] {
        &self.rules
    }

    pub fn evaluate_and_apply_rule_with_http(
        &self,
        rule_id: RuleId,
        document: &mut Document,
        mut http_matches: impl FnMut(&MatchField, &MatchOperator) -> Result<bool, DomainError>,
    ) -> Result<ConditionEvaluation, DomainError> {
        let rule = self.rule_or_error(rule_id)?;
        let evaluation = evaluate_condition(&rule.condition, document, &mut http_matches)?;
        if evaluation.matched
            && let UnifiedAction::Document(mutation) = &rule.action
        {
            mutation.apply(document)?;
        }
        Ok(evaluation)
    }

    pub fn evaluate_rule_with_http(
        &self,
        rule_id: RuleId,
        document: &Document,
        mut http_matches: impl FnMut(&MatchField, &MatchOperator) -> Result<bool, DomainError>,
    ) -> Result<ConditionEvaluation, DomainError> {
        let rule = self.rule_or_error(rule_id)?;
        evaluate_condition(&rule.condition, document, &mut http_matches)
    }

    #[must_use]
    pub fn rule(&self, rule_id: RuleId) -> Option<&RuleProgramEntry> {
        self.rules.iter().find(|rule| rule.rule_id == rule_id)
    }

    fn rule_or_error(&self, rule_id: RuleId) -> Result<&RuleProgramEntry, DomainError> {
        self.rule(rule_id)
            .ok_or_else(|| rule_error("rule_id", "规则不属于当前统一程序"))
    }

    pub fn execute_with_http(
        &self,
        document: Document,
        mut http_matches: impl FnMut(&MatchField, &MatchOperator) -> Result<bool, DomainError>,
    ) -> Result<UnifiedRuleExecution, DomainError> {
        let mut working = document;
        let mut matched_rule_ids = Vec::new();
        let mut http_actions = Vec::new();
        let mut terminal_action = None;
        'rules: for rule in &self.rules {
            if !evaluate_condition(&rule.condition, &working, &mut http_matches)?.matched {
                continue;
            }
            match &rule.action {
                UnifiedAction::Document(mutation) => mutation.apply(&mut working)?,
                UnifiedAction::RecordMatch => {}
                UnifiedAction::Http(action) => http_actions.push(action.clone()),
                UnifiedAction::Terminal(action) => {
                    terminal_action = Some(action.clone());
                    matched_rule_ids.push(rule.rule_id);
                    break 'rules;
                }
            }
            matched_rule_ids.push(rule.rule_id);
        }
        Ok(UnifiedRuleExecution {
            document: working,
            matched_rule_ids,
            http_actions,
            terminal_action,
        })
    }
}

/// Phase 5 pure-domain execution result; transaction/lifecycle commit is deliberately Phase 6.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnifiedRuleExecution {
    document: Document,
    matched_rule_ids: Vec<RuleId>,
    http_actions: Vec<HttpAction>,
    terminal_action: Option<TerminalAction>,
}

impl UnifiedRuleExecution {
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    #[must_use]
    pub fn matched_rule_ids(&self) -> &[RuleId] {
        &self.matched_rule_ids
    }

    #[must_use]
    pub fn http_actions(&self) -> &[HttpAction] {
        &self.http_actions
    }

    #[must_use]
    pub const fn terminal_action(&self) -> Option<&TerminalAction> {
        self.terminal_action.as_ref()
    }
}
