//! HTTP 与 Socket 共用的唯一规则用例。

use intercept_proxy_domain::{
    MAX_JAVASCRIPT_SAFE_INTEGER, Revision, RuleDefinition, RuleId, sort_rule_definitions,
};

use super::{Application, validation::require_confirmation};
use crate::{
    AppError, AppResult, OperationResultViewModel, RuleDefinitionSaveInput, UiTone,
    WorkspaceChangeKind,
};

impl Application {
    pub async fn rule_definition_list(&self) -> AppResult<Vec<RuleDefinition>> {
        let mut rules = self.selected_rule_workspace().await?.rule_definitions;
        sort_rule_definitions(&mut rules);
        Ok(rules)
    }

    pub async fn rule_definition_get(&self, rule_id: RuleId) -> AppResult<RuleDefinition> {
        self.selected_rule_workspace()
            .await?
            .rule_definitions
            .into_iter()
            .find(|rule| rule.rule_id() == rule_id)
            .ok_or_else(|| unified_rule_not_found(rule_id))
    }

    /// Persists an independent copy with a new identity and monotonic creation order.
    pub async fn rule_definition_copy(&self, rule_id: RuleId) -> AppResult<RuleDefinition> {
        let source = self.rule_definition_get(rule_id).await?;
        let mut draft = source.to_draft();
        draft.name = format!("{}（副本）", draft.name);
        self.rule_definition_save(RuleDefinitionSaveInput {
            rule_id: None,
            expected_revision: None,
            draft,
        })
        .await
    }

    pub async fn rule_definition_save(
        &self,
        input: RuleDefinitionSaveInput,
    ) -> AppResult<RuleDefinition> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.selected_rule_workspace().await?;
        let previous = workspace.clone();
        let listener_id = input.draft.listener_id;
        let saved_rule_id = match (input.rule_id, input.expected_revision) {
            (None, None) => {
                let created_order = workspace
                    .rule_definitions
                    .iter()
                    .map(RuleDefinition::created_order)
                    .max()
                    .unwrap_or(0)
                    .max(workspace.rule_created_order_high_water)
                    .checked_add(1)
                    .filter(|value| *value <= MAX_JAVASCRIPT_SAFE_INTEGER)
                    .ok_or_else(|| {
                        AppError::new(
                            "RULE_CREATED_ORDER_EXHAUSTED",
                            "规则创建顺序已达到可表示上限。",
                        )
                    })?;
                let rule = RuleDefinition::create(input.draft, created_order)?;
                let rule_id = rule.rule_id();
                workspace.rule_created_order_high_water = created_order;
                workspace.rule_definitions.push(rule);
                rule_id
            }
            (Some(rule_id), Some(expected_revision)) => {
                let rule = workspace
                    .rule_definitions
                    .iter_mut()
                    .find(|rule| rule.rule_id() == rule_id)
                    .ok_or_else(|| unified_rule_not_found(rule_id))?;
                rule.update(expected_revision, input.draft)?;
                rule_id
            }
            _ => {
                return Err(AppError::new(
                    "RULE_REVISION_REQUIRED",
                    "创建规则不能提供 identity/revision；更新规则必须同时提供。",
                ));
            }
        };
        let candidate = workspace
            .rule_definitions
            .iter()
            .find(|rule| rule.rule_id() == saved_rule_id)
            .ok_or_else(|| unified_rule_not_found(saved_rule_id))?;
        self.validate_rule_definition_document(&workspace, candidate)
            .await?;
        sort_rule_definitions(&mut workspace.rule_definitions);
        workspace.validate()?;
        let saved = self
            .save_rule_definitions_with_runtime_rollback(previous, workspace, listener_id)
            .await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        saved
            .rule_definitions
            .into_iter()
            .find(|rule| rule.rule_id() == saved_rule_id)
            .ok_or_else(|| unified_rule_not_found(saved_rule_id))
    }

    pub async fn rule_definition_toggle(
        &self,
        rule_id: RuleId,
        expected_revision: Revision,
        enabled: bool,
    ) -> AppResult<RuleDefinition> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.selected_rule_workspace().await?;
        let previous = workspace.clone();
        let rule = workspace
            .rule_definitions
            .iter_mut()
            .find(|rule| rule.rule_id() == rule_id)
            .ok_or_else(|| unified_rule_not_found(rule_id))?;
        let listener_id = rule.listener_id();
        rule.set_enabled(expected_revision, enabled)?;
        let result = rule.clone();
        let saved = self
            .save_rule_definitions_with_runtime_rollback(previous, workspace, listener_id)
            .await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        Ok(result)
    }

    pub async fn rule_definition_delete(
        &self,
        rule_id: RuleId,
        expected_revision: Revision,
        confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        require_confirmation(confirmed, "删除规则需要确认。")?;
        let mut workspace = self.selected_rule_workspace().await?;
        let previous = workspace.clone();
        let index = workspace
            .rule_definitions
            .iter()
            .position(|rule| rule.rule_id() == rule_id)
            .ok_or_else(|| unified_rule_not_found(rule_id))?;
        workspace.rule_definitions[index]
            .revision()
            .verify(expected_revision)?;
        let listener_id = workspace.rule_definitions[index].listener_id();
        workspace.rule_created_order_high_water = workspace
            .rule_created_order_high_water
            .max(workspace.rule_definitions[index].created_order());
        workspace.rule_definitions.remove(index);
        let saved = self
            .save_rule_definitions_with_runtime_rollback(previous, workspace, listener_id)
            .await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "规则已删除。".into(),
            ui_tone: UiTone::Positive,
            entity_id: Some(rule_id.to_string()),
            revision: Some(saved.revision.get()),
            requires_restart: false,
        })
    }

    async fn save_rule_definitions_with_runtime_rollback(
        &self,
        previous: crate::ProxyWorkspace,
        candidate: crate::ProxyWorkspace,
        listener_id: crate::ListenerId,
    ) -> AppResult<crate::ProxyWorkspace> {
        let saved = self.workspaces.save(candidate).await?;
        if let Err(replacement_error) = self
            .listener_runtime
            .replace_rule_definitions(saved.clone(), listener_id)
            .await
        {
            let mut rollback = previous;
            rollback.revision = saved.revision;
            let restored = self.workspaces.save(rollback).await.map_err(|error| {
                AppError::new(
                    "RULE_PERSISTENCE_ROLLBACK_FAILED",
                    format!(
                        "运行时替换失败，且持久化恢复失败：{}",
                        error.view_model.code
                    ),
                )
            })?;
            self.listener_runtime
                .replace_rule_definitions(restored, listener_id)
                .await
                .map_err(|error| {
                    AppError::new(
                        "RULE_RUNTIME_ROLLBACK_FAILED",
                        format!("持久化已恢复，但运行时恢复失败：{}", error.view_model.code),
                    )
                })?;
            return Err(replacement_error);
        }
        Ok(saved)
    }

    pub(super) async fn selected_rule_workspace(&self) -> AppResult<crate::ProxyWorkspace> {
        let selected = self
            .workspaces
            .list()
            .await?
            .into_iter()
            .find(|summary| summary.selected)
            .ok_or_else(|| AppError::new("WORKSPACE_NOT_SELECTED", "请先选择一个 Workspace。"))?;
        self.workspaces.get(selected.id).await
    }
}

fn unified_rule_not_found(rule_id: RuleId) -> AppError {
    AppError::new("RULE_NOT_FOUND", "当前 Workspace 中不存在指定规则。").entity(rule_id.to_string())
}
