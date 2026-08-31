use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{RuleDefinition, RuleDefinitionRestoreSnapshot, RuleLifecycle};

pub(super) fn reset_rule_lifecycle(rule: &RuleDefinition) -> AppResult<RuleDefinition> {
    RuleDefinition::restore(
        rule.rule_id(),
        rule.to_draft(),
        RuleDefinitionRestoreSnapshot {
            revision: rule.revision(),
            created_order: rule.created_order(),
            lifecycle: RuleLifecycle::default(),
        },
    )
    .map_err(AppError::from)
}
