//! 故障模板与当前故障配置的应用适配器。
//!
//! 这里校验并转换用户草稿，真正的逐消息执行留给代理 pipeline；配置错误不会改变正在
//! 使用的规则快照。

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_application::{
    ActiveFaultViewModel, AppError, AppResult, FaultConfigurationDraft,
    FaultParameterFieldViewModel, FaultParameterKind, FaultParameterValue, FaultServicePort,
    FaultTemplateViewModel, MessageStage, RuleDefinition, RuleDefinitionDraft,
    RuleDefinitionSaveInput, UiTone,
};
use intercept_proxy_domain::{
    Condition, DropResponseMode, HttpAction, HttpRuleContent, JitterScope, ListenerId, MatchField,
    MatchOperator, Revision, RuleContent, RuleId, RuleStage, TerminalAction, TrafficDirection,
};
use intercept_proxy_product_api::{BodyCodec, ProductFaultTemplate, ProductLabels, ProductProfile};
use serde_json::Value;

#[derive(Debug)]
pub struct FaultServiceAdapter {
    body_codec: Arc<dyn BodyCodec>,
    templates: &'static [ProductFaultTemplate],
    labels: ProductLabels,
}

impl FaultServiceAdapter {
    #[must_use]
    pub fn new(body_codec: Arc<dyn BodyCodec>, product: &dyn ProductProfile) -> Self {
        Self {
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

    async fn rule_draft(
        &self,
        configuration: FaultConfigurationDraft,
    ) -> AppResult<RuleDefinitionSaveInput> {
        let definition = template_definitions(self.templates)?
            .into_iter()
            .find(|template| template.view.template_id == configuration.template_id)
            .ok_or_else(|| AppError::new("RULE_INVALID", "故障模板不存在。"))?;
        let (stage, action) = definition
            .action
            .invoke(&configuration.parameters, self.body_codec.as_ref())?;
        let condition = configuration_condition(&configuration)?;
        let channel = configuration
            .channel
            .ok_or_else(|| AppError::new("RULE_INVALID", "故障规则必须绑定 Listener。"))?;
        let listener_id = uuid::Uuid::parse_str(channel.as_str())
            .map(ListenerId::from_uuid)
            .map_err(|_| AppError::new("RULE_INVALID", "故障规则 Listener ID 无效。"))?;
        Ok(RuleDefinitionSaveInput {
            rule_id: configuration.existing_rule_id.map(RuleId::from_uuid),
            expected_revision: configuration.expected_revision.map(Revision::new),
            draft: RuleDefinitionDraft {
                name: format!(
                    "{}{}",
                    self.labels.fault_rule_name_prefix, definition.view.name
                ),
                enabled: true,
                priority: configuration.priority,
                listener_id,
                stage: rule_stage(stage)?,
                content: RuleContent::Http(HttpRuleContent {
                    description: format!("fault:{}", definition.view.template_id),
                    condition,
                    action: intercept_proxy_domain::UnifiedAction::from(action),
                }),
            },
        })
    }

    fn active_view(&self, rule: &RuleDefinition) -> Option<ActiveFaultViewModel> {
        let RuleContent::Http(content) = rule.content() else {
            return None;
        };
        content.description.strip_prefix("fault:")?;
        let template_name = rule
            .name()
            .strip_prefix(self.labels.fault_rule_name_prefix)?;
        Some(ActiveFaultViewModel {
            rule_id: rule.rule_id().as_uuid(),
            template_name: template_name.into(),
            target_summary: "1 个条件".into(),
            priority: rule.priority(),
            hit_count: rule.lifecycle().hit_count,
            enabled: rule.enabled(),
            status_text: if rule.enabled() {
                "活动中".into()
            } else {
                "已停用".into()
            },
            ui_tone: if rule.enabled() {
                UiTone::Warning
            } else {
                UiTone::Neutral
            },
            revision: rule.revision().get(),
        })
    }
}

fn rule_stage(stage: MessageStage) -> AppResult<RuleStage> {
    match stage {
        MessageStage::Request => Ok(RuleStage::ProxyToUpstream),
        MessageStage::Response => Ok(RuleStage::ProxyToApp),
        MessageStage::TlsHandshake | MessageStage::Terminal => Err(AppError::new(
            "RULE_INVALID",
            "故障模板阶段不是规则写出方向。",
        )),
    }
}

fn configuration_condition(configuration: &FaultConfigurationDraft) -> AppResult<Condition> {
    let mut conditions = Vec::new();
    if let Some(terminal) = configuration
        .terminal
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        conditions.push(Condition::Http {
            field: MatchField::TerminalIp,
            operator: MatchOperator::Equals(terminal.clone()),
        });
    }
    if let Some(target) = configuration
        .target
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        conditions.push(Condition::Http {
            field: MatchField::RequestTarget,
            operator: MatchOperator::Contains(target.clone()),
        });
    }
    match conditions.as_slice() {
        [condition] => Ok(condition.clone()),
        [] => Err(AppError::new("RULE_INVALID", "故障规则必须设置一个匹配条件。")),
        _ => Err(AppError::new("RULE_INVALID", "故障规则只能设置一个匹配条件。")),
    }
}

mod actions;
mod template_fields;
mod templates;

use actions::{
    connect_timeout, custom_status, disconnect, disconnect_downstream_mid_body,
    disconnect_upstream_mid_body, drop_response, intermittent_downstream, intermittent_upstream,
    invalid_json, jitter_downstream, jitter_upstream, mock_response, modify_json, read_timeout,
    request_delay, response_delay, throttle_downstream, throttle_upstream, truncate, write_timeout,
    wrong_length,
};
use template_fields::{encoded_template, template};
#[cfg(test)]
use templates::generic_template_definitions;
use templates::{FaultParameters, TemplateAction, TemplateDefinition, template_definitions};

#[cfg(test)]
#[path = "faults_tests.rs"]
mod faults_tests;
