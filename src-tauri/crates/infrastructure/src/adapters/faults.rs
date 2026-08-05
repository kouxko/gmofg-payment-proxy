//! 故障模板与当前故障配置的应用适配器。
//!
//! 这里校验并转换用户草稿，真正的逐消息执行留给代理 pipeline；配置错误不会改变正在
//! 使用的规则快照。

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_application::{
    ActiveFaultViewModel, AppError, AppResult, FaultConfigurationDraft,
    FaultParameterFieldViewModel, FaultParameterKind, FaultParameterValue, FaultServicePort,
    FaultTemplateViewModel, MessageStage, RuleDraft, RuleRepositoryPort, UiTone,
};
use intercept_proxy_domain::{
    DropResponseMode, JitterScope, MatchCondition, MatchField, MatchOperator, RuleAction,
    TerminalAction, TrafficDirection,
};
use intercept_proxy_product_api::{BodyCodec, ProductFaultTemplate, ProductLabels, ProductProfile};
use serde_json::Value;

use super::rules::{RuleRepositoryAdapter, action_to_app, condition_to_app};

#[derive(Debug)]
pub struct FaultServiceAdapter {
    rules: Arc<RuleRepositoryAdapter>,
    body_codec: Arc<dyn BodyCodec>,
    templates: &'static [ProductFaultTemplate],
    labels: ProductLabels,
}

impl FaultServiceAdapter {
    #[must_use]
    pub fn new(
        rules: Arc<RuleRepositoryAdapter>,
        body_codec: Arc<dyn BodyCodec>,
        product: &dyn ProductProfile,
    ) -> Self {
        Self {
            rules,
            body_codec,
            templates: product.fault_templates(),
            labels: product.labels(),
        }
    }
}

#[async_trait]
impl FaultServicePort for FaultServiceAdapter {
    async fn templates(&self) -> AppResult<Vec<FaultTemplateViewModel>> {
        Ok(template_definitions(self.templates)?
            .into_iter()
            .map(|template| template.view)
            .collect())
    }

    async fn configure(
        &self,
        configuration: FaultConfigurationDraft,
    ) -> AppResult<ActiveFaultViewModel> {
        let definition = template_definitions(self.templates)?
            .into_iter()
            .find(|template| template.view.template_id == configuration.template_id)
            .ok_or_else(|| AppError::new("RULE_INVALID", "故障模板不存在。"))?;
        let (stage, action) = definition
            .action
            .invoke(&configuration.parameters, self.body_codec.as_ref())?;
        let conditions = configuration_conditions(&configuration, stage)?;
        let rule = self
            .rules
            .save(RuleDraft {
                rule_id: configuration.existing_rule_id,
                expected_revision: configuration.expected_revision,
                name: format!(
                    "{}{}",
                    self.labels.fault_rule_name_prefix, definition.view.name
                ),
                description: format!("fault:{}", definition.view.template_id),
                enabled: true,
                priority: configuration.priority,
                channel: configuration.channel,
                stage: Some(stage),
                conditions: conditions.iter().map(condition_to_app).collect(),
                actions: vec![
                    action_to_app(&action)
                        .map_err(|error| AppError::new("RULE_INVALID", error.to_string()))?,
                ],
                one_shot: configuration.one_shot,
            })
            .await?;
        Ok(active_from_rule(&rule, &definition.view.name))
    }

    async fn active(&self) -> AppResult<Vec<ActiveFaultViewModel>> {
        let rules = self.rules.list().await?;
        Ok(rules
            .into_iter()
            .filter(|rule| rule.name.starts_with(self.labels.fault_rule_name_prefix))
            .map(|rule| ActiveFaultViewModel {
                rule_id: rule.rule_id,
                template_name: rule
                    .name
                    .trim_start_matches(self.labels.fault_rule_name_prefix)
                    .into(),
                target_summary: rule.match_summary,
                priority: rule.priority,
                hit_count: rule.hit_count,
                enabled: rule.enabled,
                status_text: if rule.enabled {
                    "活动中".into()
                } else {
                    "已停用".into()
                },
                ui_tone: if rule.enabled {
                    UiTone::Warning
                } else {
                    UiTone::Neutral
                },
                revision: rule.revision,
            })
            .collect())
    }

    async fn stop(
        &self,
        rule_id: intercept_proxy_application::RuleId,
        expected_revision: u64,
    ) -> AppResult<ActiveFaultViewModel> {
        let rule = self.rules.toggle(rule_id, expected_revision, false).await?;
        Ok(active_from_rule(
            &rule,
            rule.summary
                .name
                .trim_start_matches(self.labels.fault_rule_name_prefix),
        ))
    }
}

fn configuration_conditions(
    configuration: &FaultConfigurationDraft,
    stage: MessageStage,
) -> AppResult<Vec<MatchCondition>> {
    if stage == MessageStage::TlsHandshake {
        let mut field_errors = BTreeMap::new();
        if configuration
            .terminal
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            field_errors.insert(
                "terminal".into(),
                vec!["TLS 握手阶段不能按终端 IP 匹配，请在规则页面使用客户端证书指纹。".into()],
            );
        }
        if configuration
            .target
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            field_errors.insert(
                "target".into(),
                vec!["TLS 握手阶段尚未解析 HTTP 路径，不能配置路径条件。".into()],
            );
        }
        if !field_errors.is_empty() {
            return Err(AppError::field(
                "RULE_INVALID",
                "TLS 握手故障包含不支持的匹配条件。",
                field_errors,
            ));
        }
        return Ok(configuration
            .nth_hit
            .map(|nth| vec![MatchCondition::NthHit(u64::from(nth))])
            .unwrap_or_default());
    }

    let mut conditions = Vec::new();
    if let Some(terminal) = configuration
        .terminal
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        conditions.push(MatchCondition::Field {
            field: MatchField::TerminalIp,
            operator: MatchOperator::Equals(terminal.clone()),
        });
    }
    if let Some(target) = configuration
        .target
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        conditions.push(MatchCondition::Field {
            field: MatchField::PathOrRequestType,
            operator: MatchOperator::Contains(target.clone()),
        });
    }
    if let Some(nth) = configuration.nth_hit {
        conditions.push(MatchCondition::NthHit(u64::from(nth)));
    }
    Ok(conditions)
}

mod actions;
mod template_fields;
mod templates;

use actions::{
    active_from_rule, connect_timeout, custom_status, disconnect, disconnect_downstream_mid_body,
    disconnect_upstream_mid_body, drop_response, intermittent_downstream, intermittent_upstream,
    invalid_json, jitter_downstream, jitter_upstream, mock_response, modify_json, read_timeout,
    reject_tls, request_delay, response_delay, throttle_downstream, throttle_upstream, truncate,
    write_timeout, wrong_length,
};
use template_fields::{encoded_template, template};
#[cfg(test)]
use templates::generic_template_definitions;
use templates::{FaultParameters, TemplateAction, TemplateDefinition, template_definitions};

#[cfg(test)]
#[path = "faults_tests.rs"]
mod faults_tests;
