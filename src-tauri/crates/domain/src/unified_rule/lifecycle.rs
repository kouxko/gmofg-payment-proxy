use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{DomainError, Revision, RuleId};

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
        if !has_hit {
            return Err(rule_binding_error("lifecycle", "生命周期增量不得为空"));
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
        Ok(candidate)
    }
}
