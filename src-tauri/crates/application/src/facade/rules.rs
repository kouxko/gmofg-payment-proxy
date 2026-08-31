//! 规则编辑、校验、持久化与故障模板用例。
//!
//! 规则输入解析和修改保护集中在这里，与生命周期、流量和设置流程隔离；所有展示适配器
//! 仍通过稳定的 [`Application`] API 调用。

mod exchange_mock;
use super::{Application, validation::require_confirmation};
use crate::{
    ActiveFaultViewModel, AppError, AppResult, FaultConfigurationDraft, FaultTemplateViewModel,
    RuleId,
};

impl Application {
    pub async fn fault_template_list(&self) -> AppResult<Vec<FaultTemplateViewModel>> {
        let mut templates = self.faults.templates().await?;
        let channel = self
            .selected_workspace_channel_catalog()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                AppError::new(
                    "LISTENER_REQUIRED",
                    "当前 Workspace 没有代理入口；请先新增入口再配置故障模拟。",
                )
            })?
            .id;
        for template in &mut templates {
            template.default_channel = channel.clone();
        }
        templates.sort_by(|left, right| left.template_id.cmp(&right.template_id));
        Ok(templates)
    }

    pub async fn fault_configure(
        &self,
        draft: FaultConfigurationDraft,
    ) -> AppResult<ActiveFaultViewModel> {
        let input = self.faults.rule_draft(draft).await?;
        let saved = self.rule_definition_save(input).await?;
        self.faults.active_view(&saved).ok_or_else(|| {
            AppError::new("RULE_INVALID", "故障模板生成的统一规则无法投影为活动故障。")
        })
    }

    pub async fn fault_active_list(&self) -> AppResult<Vec<ActiveFaultViewModel>> {
        let mut active = self
            .rule_definition_list()
            .await?
            .iter()
            .filter_map(|rule| self.faults.active_view(rule))
            .collect::<Vec<_>>();
        active.sort_by_key(|fault| (fault.priority, fault.rule_id));
        Ok(active)
    }

    pub async fn fault_stop(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<ActiveFaultViewModel> {
        require_confirmation(confirmed, "停止活动故障需要确认。")?;
        let saved = self
            .rule_definition_toggle(
                intercept_proxy_domain::RuleId::from_uuid(rule_id),
                intercept_proxy_domain::Revision::new(expected_revision),
                false,
            )
            .await?;
        self.faults
            .active_view(&saved)
            .ok_or_else(|| AppError::new("RULE_NOT_FOUND", "指定规则不是活动故障规则。"))
    }
}
