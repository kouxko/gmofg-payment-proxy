use super::{AppError, AppResult, Rule, RuleRuntimeSnapshot};
#[cfg(test)]
use super::{
    AppMessageStage, AppRuleAction, AppRuleCondition, AppRuleDraft, AppRuleMatchField,
    AppRuleMatchOperator, BTreeMap, ChannelId, FieldValidationViewModel, MatchCondition,
    MatchField, MatchOperator, MessageStage, Revision, RuleDraft, RuleSummaryViewModel,
    RuleValidationViewModel, RuleViewModel, UiTone, action_to_app, action_to_domain, json_error,
};

pub(super) fn runtime_rules(
    snapshot: &RuleRuntimeSnapshot,
    evaluated_rules: &[Rule],
) -> AppResult<Vec<Rule>> {
    let mut expected_ids = snapshot
        .rules
        .iter()
        .map(|rule| rule.id)
        .collect::<Vec<_>>();
    let mut evaluated_ids = evaluated_rules
        .iter()
        .map(|rule| rule.id)
        .collect::<Vec<_>>();
    expected_ids.sort_unstable();
    evaluated_ids.sort_unstable();
    if evaluated_ids != expected_ids {
        return Err(AppError::new(
            "RULE_INVALID",
            "运行态提交不得增加、删除或重复规则。",
        ));
    }
    snapshot
        .rules
        .iter()
        .map(|original| {
            let evaluated = evaluated_rules
                .iter()
                .find(|rule| rule.id == original.id)
                .ok_or_else(|| AppError::new("RULE_INVALID", "运行态提交缺少规则。"))?;
            let one_shot_fired = original.one_shot
                && original.enabled
                && !evaluated.enabled
                && evaluated.revision == original.revision.next();
            let mut allowed = original.clone();
            allowed.hit_count = evaluated.hit_count;
            allowed.last_hit_at = evaluated.last_hit_at;
            if one_shot_fired {
                allowed.enabled = evaluated.enabled;
                allowed.revision = evaluated.revision;
            }
            if allowed != *evaluated {
                return Err(AppError::new(
                    "RULE_INVALID",
                    "运行态提交包含非命中元数据的配置变更。",
                )
                .entity(original.id.to_string()));
            }
            Ok(evaluated.clone())
        })
        .collect()
}

#[cfg(test)]
pub(super) fn to_domain_draft(
    draft: &AppRuleDraft,
    creation_order: u64,
) -> Result<RuleDraft, intercept_proxy_domain::DomainError> {
    let stage = match draft.stage {
        Some(AppMessageStage::Request) => MessageStage::Request,
        Some(AppMessageStage::Response) => MessageStage::Response,
        Some(AppMessageStage::TlsHandshake) => MessageStage::TlsHandshake,
        _ => {
            return Err(intercept_proxy_domain::DomainError::new(
                intercept_proxy_domain::ErrorCode::RuleInvalid,
                "规则必须指定 TLS 握手、请求或响应阶段",
            )
            .with_field_error("stage", "阶段无效"));
        }
    };
    let priority = u32::try_from(draft.priority).map_err(|_| {
        intercept_proxy_domain::DomainError::new(
            intercept_proxy_domain::ErrorCode::RuleInvalid,
            "规则优先级不能为负数",
        )
        .with_field_error("priority", "必须大于等于 0")
    })?;
    let conditions = draft.conditions.iter().map(condition_to_domain).collect();
    let actions = draft
        .actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            action_to_domain(action).map_err(|error| {
                let (field, message) = match action {
                    AppRuleAction::SetJsonField { value_json, .. }
                        if value_json.trim().is_empty() =>
                    {
                        (
                            format!("actions.{index}.value_json"),
                            format!(
                                "动作 {} 的 JSON 值不能为空；请输入 null、字符串、数字、对象或数组",
                                index + 1
                            ),
                        )
                    }
                    AppRuleAction::SetJsonField { .. } => (
                        format!("actions.{index}.value_json"),
                        format!(
                            "动作 {} 的 JSON 值格式无效；第 {} 行第 {} 列附近存在错误",
                            index + 1,
                            error.line(),
                            error.column()
                        ),
                    ),
                    _ => (
                        format!("actions.{index}"),
                        format!("动作 {} 的参数格式无效", index + 1),
                    ),
                };
                intercept_proxy_domain::DomainError::new(
                    intercept_proxy_domain::ErrorCode::RuleInvalid,
                    "规则动作无效",
                )
                .with_field_error(field, message)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RuleDraft {
        expected_revision: draft.expected_revision.map(Revision::new),
        name: draft.name.clone(),
        description: draft.description.clone(),
        enabled: draft.enabled,
        priority,
        created_order: creation_order,
        channel: draft.channel.clone(),
        stage,
        conditions,
        actions,
        one_shot: draft.one_shot,
    })
}

#[cfg(test)]
pub(super) fn app_draft(rule: &Rule) -> AppResult<AppRuleDraft> {
    Ok(AppRuleDraft {
        rule_id: Some(rule.id.as_uuid()),
        expected_revision: Some(rule.revision.get()),
        name: rule.name.clone(),
        description: rule.description.clone(),
        enabled: rule.enabled,
        priority: i32::try_from(rule.priority)
            .map_err(|error| json_error("规则优先级超出 UI 范围", error))?,
        channel: rule.channel.clone(),
        stage: Some(match rule.stage {
            MessageStage::Request => AppMessageStage::Request,
            MessageStage::Response => AppMessageStage::Response,
            MessageStage::TlsHandshake => AppMessageStage::TlsHandshake,
        }),
        conditions: rule.conditions.iter().map(condition_to_app).collect(),
        actions: rule
            .actions
            .iter()
            .map(action_to_app)
            .collect::<Result<_, _>>()
            .map_err(|error| json_error("规则动作转换失败", error))?,
        one_shot: rule.one_shot,
    })
}

#[cfg(test)]
pub(crate) fn condition_to_domain(condition: &AppRuleCondition) -> MatchCondition {
    match condition {
        AppRuleCondition::Field { field, operator } => MatchCondition::Field {
            field: match field {
                AppRuleMatchField::TerminalIp => MatchField::TerminalIp,
                AppRuleMatchField::CertificateFingerprint => MatchField::CertificateFingerprint,
                AppRuleMatchField::PathOrRequestType => MatchField::PathOrRequestType,
                AppRuleMatchField::JsonPath { path } => MatchField::JsonPath(path.clone()),
            },
            operator: match operator {
                AppRuleMatchOperator::Equals { value } => MatchOperator::Equals(value.clone()),
                AppRuleMatchOperator::Contains { value } => MatchOperator::Contains(value.clone()),
                AppRuleMatchOperator::Regex { pattern } => MatchOperator::Regex(pattern.clone()),
            },
        },
        AppRuleCondition::NthHit { count } => MatchCondition::NthHit(*count),
    }
}

#[cfg(test)]
pub(crate) fn condition_to_app(condition: &MatchCondition) -> AppRuleCondition {
    match condition {
        MatchCondition::Field { field, operator } => AppRuleCondition::Field {
            field: match field {
                MatchField::TerminalIp => AppRuleMatchField::TerminalIp,
                MatchField::CertificateFingerprint => AppRuleMatchField::CertificateFingerprint,
                MatchField::PathOrRequestType => AppRuleMatchField::PathOrRequestType,
                MatchField::JsonPath(path) => AppRuleMatchField::JsonPath { path: path.clone() },
            },
            operator: match operator {
                MatchOperator::Equals(value) => AppRuleMatchOperator::Equals {
                    value: value.clone(),
                },
                MatchOperator::Contains(value) => AppRuleMatchOperator::Contains {
                    value: value.clone(),
                },
                MatchOperator::Regex(pattern) => AppRuleMatchOperator::Regex {
                    pattern: pattern.clone(),
                },
            },
        },
        MatchCondition::NthHit(count) => AppRuleCondition::NthHit { count: *count },
    }
}

#[cfg(test)]
pub(super) fn summary(
    rule: &Rule,
    channel_names: &BTreeMap<ChannelId, String>,
) -> AppResult<RuleSummaryViewModel> {
    Ok(RuleSummaryViewModel {
        rule_id: rule.id.as_uuid(),
        revision: rule.revision.get(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        priority: i32::try_from(rule.priority)
            .map_err(|error| json_error("规则优先级超出 UI 范围", error))?,
        creation_order: rule.created_order,
        channel_text: rule.channel.as_ref().map_or_else(
            || "全部".into(),
            |channel| {
                channel_names
                    .get(channel)
                    .cloned()
                    .unwrap_or_else(|| channel.to_string())
            },
        ),
        stage_text: match rule.stage {
            MessageStage::Request => "请求".into(),
            MessageStage::Response => "响应".into(),
            MessageStage::TlsHandshake => "TLS 握手".into(),
        },
        match_summary: format!("{} 个条件", rule.conditions.len()),
        action_summary: format!("{} 个动作", rule.actions.len()),
        hit_count: rule.hit_count,
        last_hit_at: rule.last_hit_at,
        ui_tone: if rule.enabled {
            UiTone::Positive
        } else {
            UiTone::Neutral
        },
    })
}

#[cfg(test)]
pub(super) fn view(
    rule: &Rule,
    channel_names: &BTreeMap<ChannelId, String>,
) -> AppResult<RuleViewModel> {
    Ok(RuleViewModel {
        summary: summary(rule, channel_names)?,
        draft: app_draft(rule)?,
    })
}

#[cfg(test)]
pub(super) fn validation_from_domain(
    error: &intercept_proxy_domain::DomainError,
) -> RuleValidationViewModel {
    FieldValidationViewModel {
        valid: false,
        field_errors: error
            .field_errors
            .iter()
            .map(|(field, messages)| (field.clone(), messages.clone()))
            .collect(),
        warnings: Vec::new(),
    }
}
