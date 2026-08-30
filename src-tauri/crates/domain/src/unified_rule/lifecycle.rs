use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{DomainError, Revision, RuleId, TerminalIdentity};

use super::{RuleDefinition, rule_binding_error};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleDefinitionRestoreSnapshot {
    pub revision: Revision,
    pub created_order: u64,
    pub lifecycle: RuleLifecycle,
}

/// Lifecycle shared by HTTP and Socket rule definitions.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleLifecycle {
    pub hit_count: u64,
    pub last_hit_at: Option<DateTime<Utc>>,
}

/// Immutable lifecycle baseline used by one transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleLifecycleSnapshot {
    pub rule_id: RuleId,
    pub revision: Revision,
    pub enabled: bool,
    pub one_shot: bool,
    pub lifecycle: RuleLifecycle,
}

impl RuleLifecycleSnapshot {
    #[must_use]
    pub fn delta_for_successful_match(&self, last_hit_at: DateTime<Utc>) -> RuleLifecycleDelta {
        RuleLifecycleDelta {
            rule_id: self.rule_id,
            expected_revision: self.revision,
            hit_count_increment: 1,
            last_hit_at: Some(last_hit_at),
            disable_one_shot: self.one_shot && self.enabled,
            nth_counter_advance: None,
        }
    }
}

/// A tentative lifecycle change committed with revision compare-and-set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct RuleLifecycleDelta {
    pub rule_id: RuleId,
    pub expected_revision: Revision,
    pub hit_count_increment: u64,
    pub last_hit_at: Option<DateTime<Utc>>,
    pub disable_one_shot: bool,
    pub nth_counter_advance: Option<NthCounterAdvance>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct NthCounterSnapshot {
    pub rule_id: RuleId,
    pub terminal: TerminalIdentity,
    pub attempts: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct NthCounterAdvance {
    pub rule_id: RuleId,
    pub terminal: TerminalIdentity,
    pub expected_attempts: u64,
    pub increment: u64,
}

impl RuleLifecycleDelta {
    pub fn validate_against(&self, snapshot: &RuleLifecycleSnapshot) -> Result<(), DomainError> {
        if self.rule_id != snapshot.rule_id {
            return Err(rule_binding_error("rule_id", "生命周期增量不属于当前规则"));
        }
        snapshot.revision.verify(self.expected_revision)?;
        if self.hit_count_increment > 1 {
            return Err(rule_binding_error(
                "hit_count_increment",
                "命中次数每次只能增加 1",
            ));
        }
        let has_hit = self.hit_count_increment == 1;
        if has_hit != self.last_hit_at.is_some() {
            return Err(rule_binding_error(
                "last_hit_at",
                "命中增量与 last_hit_at 必须同时存在",
            ));
        }
        if let Some(advance) = &self.nth_counter_advance
            && (advance.rule_id != snapshot.rule_id || advance.increment != 1)
        {
            return Err(rule_binding_error(
                "nth_counter_advance",
                "Nth counter 增量无效",
            ));
        }
        if !has_hit && self.nth_counter_advance.is_none() {
            return Err(rule_binding_error("lifecycle", "生命周期增量不得为空"));
        }
        if self.disable_one_shot && !has_hit {
            return Err(rule_binding_error(
                "disable_one_shot",
                "one-shot 只能由成功命中禁用",
            ));
        }
        if self.disable_one_shot && (!snapshot.one_shot || !snapshot.enabled) {
            return Err(rule_binding_error("lifecycle", "one-shot 生命周期增量无效"));
        }
        Ok(())
    }
}

impl RuleDefinition {
    #[must_use]
    pub const fn lifecycle(&self) -> &RuleLifecycle {
        &self.lifecycle
    }

    #[must_use]
    pub fn lifecycle_snapshot(&self) -> RuleLifecycleSnapshot {
        RuleLifecycleSnapshot {
            rule_id: self.rule_id,
            revision: self.revision,
            enabled: self.enabled,
            one_shot: self.one_shot,
            lifecycle: self.lifecycle.clone(),
        }
    }

    #[must_use]
    pub fn lifecycle_delta_for_successful_match(
        &self,
        last_hit_at: DateTime<Utc>,
    ) -> RuleLifecycleDelta {
        self.lifecycle_snapshot()
            .delta_for_successful_match(last_hit_at)
    }

    pub fn apply_lifecycle_delta(&self, delta: &RuleLifecycleDelta) -> Result<Self, DomainError> {
        delta.validate_against(&self.lifecycle_snapshot())?;
        let mut candidate = self.clone();
        if let Some(last_hit_at) = delta.last_hit_at {
            candidate.lifecycle.hit_count = candidate
                .lifecycle
                .hit_count
                .checked_add(delta.hit_count_increment)
                .ok_or_else(|| rule_binding_error("hit_count", "规则命中次数溢出"))?;
            candidate.lifecycle.last_hit_at = Some(last_hit_at);
        }
        if delta.disable_one_shot {
            candidate.enabled = false;
            candidate.revision = candidate.revision.checked_next()?;
        }
        Ok(candidate)
    }
}
