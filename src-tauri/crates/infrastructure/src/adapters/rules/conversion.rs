use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{RuleLifecycleDelta, RuleRuntimeSnapshot, RuleSetSignature};

pub(crate) fn apply_runtime_deltas(
    snapshot: &RuleRuntimeSnapshot,
    deltas: &[RuleLifecycleDelta],
) -> AppResult<Vec<intercept_proxy_domain::RuleDefinition>> {
    if RuleSetSignature::from_definitions(&snapshot.rules) != snapshot.signature {
        return Err(AppError::new(
            "REVISION_CONFLICT",
            "规则运行快照签名与内容不一致。",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for delta in deltas {
        if !seen.insert(delta.rule_id) {
            return Err(AppError::new(
                "RULE_INVALID",
                "生命周期增量不得重复 rule_id。",
            ));
        }
        let rule = snapshot
            .rules
            .iter()
            .find(|rule| rule.rule_id() == delta.rule_id)
            .ok_or_else(|| AppError::new("RULE_INVALID", "生命周期增量引用未知规则。"))?;
        delta
            .validate_against(&rule.lifecycle_snapshot())
            .map_err(AppError::from)?;
        rule.lifecycle()
            .hit_count
            .checked_add(delta.hit_count_increment)
            .ok_or_else(|| AppError::new("RULE_INVALID", "规则命中次数溢出。"))?;
    }
    let mut rules = snapshot.rules.clone();
    for delta in deltas {
        let index = rules
            .iter()
            .position(|rule| rule.rule_id() == delta.rule_id)
            .ok_or_else(|| AppError::new("RULE_INVALID", "生命周期增量引用未知规则。"))?;
        rules[index] = rules[index]
            .apply_lifecycle_delta(delta)
            .map_err(AppError::from)?;
    }
    Ok(rules)
}
