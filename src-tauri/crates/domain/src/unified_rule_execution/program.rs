use std::collections::BTreeSet;

use super::{
    ConditionEvaluation, ConditionTree, Document, DomainError, HttpAction, MatchField,
    MatchOperator, RuleId, TerminalAction, UnifiedAction, rule_error,
};

/// Immutable rule program input. `created_order` is history/UI metadata only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleProgramEntry {
    rule_id: RuleId,
    priority: i32,
    created_order: u64,
    condition: ConditionTree,
    actions: Vec<UnifiedAction>,
}

impl RuleProgramEntry {
    /// Constructs a validated, independent rule program entry.
    pub fn new(
        rule_id: RuleId,
        priority: i32,
        created_order: u64,
        condition: ConditionTree,
        actions: Vec<UnifiedAction>,
    ) -> Result<Self, DomainError> {
        condition.validate()?;
        validate_actions(&actions)?;
        Ok(Self {
            rule_id,
            priority,
            created_order,
            condition,
            actions,
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
    pub const fn condition(&self) -> &ConditionTree {
        &self.condition
    }

    #[must_use]
    pub fn actions(&self) -> &[UnifiedAction] {
        &self.actions
    }

    pub fn replace_condition(&mut self, condition: ConditionTree) -> Result<(), DomainError> {
        condition.validate()?;
        self.condition = condition;
        Ok(())
    }
}

fn validate_actions(actions: &[UnifiedAction]) -> Result<(), DomainError> {
    if actions.is_empty() {
        return Err(rule_error("actions", "动作列表不能为空"));
    }
    let mut terminal = None;
    for (index, action) in actions.iter().enumerate() {
        if matches!(action, UnifiedAction::Http(HttpAction::Terminal(_))) {
            return Err(rule_error(
                &format!("actions.{index}"),
                "终止动作必须使用统一 terminal 变体",
            ));
        }
        if matches!(action, UnifiedAction::Terminal(_))
            && (terminal.replace(index).is_some() || index + 1 != actions.len())
        {
            return Err(rule_error(
                &format!("actions.{index}"),
                "终止动作至多一个且必须位于动作列表末尾",
            ));
        }
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
            rule.condition.validate()?;
            validate_actions(&rule.actions)?;
        }
        rules.sort_by_key(|rule| (rule.priority, rule.rule_id));
        Ok(Self { rules })
    }

    pub fn execute(&self, document: Document) -> Result<UnifiedRuleExecution, DomainError> {
        self.execute_with_http(document, |_, _| {
            Err(rule_error(
                "condition",
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
        nth_attempt: u64,
        mut http_matches: impl FnMut(&MatchField, &MatchOperator) -> Result<bool, DomainError>,
    ) -> Result<ConditionEvaluation, DomainError> {
        let Some(rule) = self.rules.iter().find(|rule| rule.rule_id == rule_id) else {
            return Ok(ConditionEvaluation {
                matched: true,
                eligible_without_nth: true,
                contains_nth: false,
            });
        };
        let evaluation =
            rule.condition
                .evaluate_with_nth(document, nth_attempt, &mut http_matches)?;
        if evaluation.matched {
            for action in &rule.actions {
                if let UnifiedAction::Document(mutation) = action {
                    mutation.apply(document)?;
                }
            }
        }
        Ok(evaluation)
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
            if !rule
                .condition
                .matches_with(&working, 1, &mut http_matches)?
            {
                continue;
            }
            for action in &rule.actions {
                match action {
                    UnifiedAction::Document(mutation) => mutation.apply(&mut working)?,
                    UnifiedAction::RecordMatch => {}
                    UnifiedAction::Http(action) => http_actions.push(action.clone()),
                    UnifiedAction::Terminal(action) => {
                        terminal_action = Some(action.clone());
                        matched_rule_ids.push(rule.rule_id);
                        break 'rules;
                    }
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
