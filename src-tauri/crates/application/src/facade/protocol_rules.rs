//! 当前 Workspace 的 Schema Document 规则用例。
//!
//! 查询和写入都从入口的精确协议包绑定重新取得编译描述；任何包、Schema、方向或
//! 编译缓存身份不一致都 fail-closed，前端能力目录不能替代保存门禁。

use intercept_proxy_domain::{MAX_JAVASCRIPT_SAFE_INTEGER, Revision, sort_protocol_document_rules};

mod editor_context;

use editor_context::capability_catalog;

use super::{Application, validation::require_confirmation};
use crate::{
    AppError, AppResult, ListenerId, OperationResultViewModel, ProtocolDocumentRuleDefinition,
    ProtocolDocumentRuleDraft, ProtocolDocumentRuleId, ProtocolPackageRef,
    ProtocolRuleCapabilityCatalog, ProtocolRuleSaveInput, ProtocolRuleStage, ProxyListener,
    ProxyWorkspace, UiTone, WorkspaceChangeKind,
};

impl Application {
    /// 返回当前选中 Workspace 的规则，并按运行时确定顺序稳定排序。
    pub async fn protocol_rule_list(&self) -> AppResult<Vec<ProtocolDocumentRuleDefinition>> {
        let mut rules = self
            .selected_protocol_rule_workspace()
            .await?
            .protocol_rules;
        sort_protocol_document_rules(&mut rules);
        Ok(rules)
    }

    /// 从精确入口绑定、编译描述与处理阶段生成无 HTTP 字段的能力目录。
    pub async fn protocol_rule_capabilities(
        &self,
        listener_id: ListenerId,
        stage: ProtocolRuleStage,
    ) -> AppResult<ProtocolRuleCapabilityCatalog> {
        let workspace = self.selected_protocol_rule_workspace().await?;
        let listener = find_listener(&workspace, listener_id)?;
        let context = self.protocol_rule_context(listener, stage).await?;
        Ok(capability_catalog(&context, stage))
    }

    /// 新建或乐观锁更新规则。更新不能改变 Listener/包/Schema/方向绑定。
    pub async fn protocol_rule_save(
        &self,
        input: ProtocolRuleSaveInput,
    ) -> AppResult<ProtocolDocumentRuleDefinition> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.selected_protocol_rule_workspace().await?;
        let previous_workspace = workspace.clone();

        // 更新先验证本地实体、revision 与冻结绑定，不能让伪造绑定提前触发另一个包的编译查询。
        match (input.rule_id, input.expected_revision) {
            (Some(rule_id), Some(expected_revision)) => {
                let current = workspace
                    .protocol_rules
                    .iter()
                    .find(|rule| rule.rule_id() == rule_id)
                    .ok_or_else(|| protocol_rule_not_found(rule_id))?;
                current
                    .revision()
                    .verify(Revision::new(expected_revision))?;
                ensure_immutable_binding(current, &input)?;
            }
            (None, None) => {}
            _ => {
                return Err(AppError::new(
                    "PROTOCOL_RULE_REVISION_REQUIRED",
                    "创建规则不能提供 revision；更新规则必须同时提供规则 ID 与期望 revision。",
                ));
            }
        }

        let listener = find_listener(&workspace, input.listener_id)?.clone();
        let context = self.protocol_rule_context(&listener, input.stage).await?;
        ensure_requested_binding(&input, &context.package, context.schema.version())?;

        let saved_rule_id = match (input.rule_id, input.expected_revision) {
            (None, None) => {
                let created_order = next_created_order(
                    &workspace.protocol_rules,
                    workspace.protocol_rule_created_order_high_water,
                )?;
                let rule_id = ProtocolDocumentRuleId::new();
                let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
                    rule_id,
                    input.name,
                    input.enabled,
                    input.priority,
                    created_order,
                    input.listener_id,
                    input.package,
                    input.schema_version,
                    input.stage,
                    input.conditions,
                    input.actions,
                )?;
                rule.validate_against_schema(&context.schema)?;
                workspace.protocol_rule_created_order_high_water = created_order;
                workspace.protocol_rules.push(rule);
                rule_id
            }
            (Some(rule_id), Some(expected_revision)) => {
                let current = workspace
                    .protocol_rules
                    .iter_mut()
                    .find(|rule| rule.rule_id() == rule_id)
                    .ok_or_else(|| protocol_rule_not_found(rule_id))?;
                ensure_immutable_binding(current, &input)?;
                let values = ProtocolDocumentRuleDraft {
                    name: input.name,
                    enabled: input.enabled,
                    priority: input.priority,
                    listener_id: input.listener_id,
                    package: input.package,
                    schema_version: input.schema_version,
                    stage: input.stage,
                    conditions: input.conditions,
                    actions: input.actions,
                };
                current.update(Revision::new(expected_revision), values)?;
                current.validate_against_schema(&context.schema)?;
                rule_id
            }
            _ => unreachable!("rule ID/revision pair was validated before external queries"),
        };

        sort_protocol_document_rules(&mut workspace.protocol_rules);
        workspace.validate()?;
        let saved = self
            .save_protocol_rules_with_runtime_rollback(
                previous_workspace,
                workspace,
                input.listener_id,
            )
            .await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        saved
            .protocol_rules
            .into_iter()
            .find(|rule| rule.rule_id() == saved_rule_id)
            .ok_or_else(|| protocol_rule_not_found(saved_rule_id))
    }

    pub async fn protocol_rule_toggle(
        &self,
        rule_id: ProtocolDocumentRuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<ProtocolDocumentRuleDefinition> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.selected_protocol_rule_workspace().await?;
        let previous_workspace = workspace.clone();
        let current = workspace
            .protocol_rules
            .iter_mut()
            .find(|rule| rule.rule_id() == rule_id)
            .ok_or_else(|| protocol_rule_not_found(rule_id))?;
        current.set_enabled(Revision::new(expected_revision), enabled)?;
        let result = current.clone();
        sort_protocol_document_rules(&mut workspace.protocol_rules);
        let saved = self
            .save_protocol_rules_with_runtime_rollback(
                previous_workspace,
                workspace,
                result.listener_id(),
            )
            .await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        Ok(result)
    }

    pub async fn protocol_rule_delete(
        &self,
        rule_id: ProtocolDocumentRuleId,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        require_confirmation(confirmed, "删除 协议报文规则需要确认。")?;
        let mut workspace = self.selected_protocol_rule_workspace().await?;
        let previous_workspace = workspace.clone();
        let index = workspace
            .protocol_rules
            .iter()
            .position(|rule| rule.rule_id() == rule_id)
            .ok_or_else(|| protocol_rule_not_found(rule_id))?;
        workspace.protocol_rules[index]
            .revision()
            .verify(Revision::new(expected_revision))?;
        workspace.protocol_rule_created_order_high_water = workspace
            .protocol_rule_created_order_high_water
            .max(workspace.protocol_rules[index].created_order());
        let listener_id = workspace.protocol_rules[index].listener_id();
        workspace.protocol_rules.remove(index);
        let saved = self
            .save_protocol_rules_with_runtime_rollback(previous_workspace, workspace, listener_id)
            .await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "协议报文规则已删除。".into(),
            ui_tone: UiTone::Positive,
            entity_id: Some(rule_id.to_string()),
            revision: Some(saved.revision.get()),
            requires_restart: false,
        })
    }

    async fn save_protocol_rules_with_runtime_rollback(
        &self,
        previous_workspace: ProxyWorkspace,
        candidate_workspace: ProxyWorkspace,
        listener_id: ListenerId,
    ) -> AppResult<ProxyWorkspace> {
        let saved = self.workspaces.save(candidate_workspace).await?;
        let replacement_error = match self
            .listener_runtime
            .replace_protocol_rules(saved.clone(), listener_id)
            .await
        {
            Ok(()) => return Ok(saved),
            Err(error) => error,
        };

        // 运行时替换失败时，把刚写入的 revision 作为补偿写入的乐观锁基线，恢复完整旧聚合。
        // mutation_gate 仍由调用方持有，因此同一进程内没有其他配置写入能插入两次保存之间。
        let mut rollback = previous_workspace;
        rollback.revision = saved.revision;
        let restored = self.workspaces.save(rollback).await.map_err(|error| {
            AppError::new(
                "PROTOCOL_RULE_COMMIT_ROLLBACK_FAILED",
                format!(
                    "规则运行时更新失败，且持久化配置恢复失败：{} / {}",
                    replacement_error.view_model.code, error.view_model.code
                ),
            )
            .entity(listener_id.to_string())
        })?;
        self.listener_runtime
            .replace_protocol_rules(restored, listener_id)
            .await
            .map_err(|error| {
                AppError::new(
                    "PROTOCOL_RULE_RUNTIME_ROLLBACK_FAILED",
                    format!(
                        "规则持久化配置已恢复，但运行时恢复失败：{}",
                        error.view_model.code
                    ),
                )
                .entity(listener_id.to_string())
            })?;
        Err(replacement_error)
    }

    async fn selected_protocol_rule_workspace(&self) -> AppResult<ProxyWorkspace> {
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

fn find_listener(workspace: &ProxyWorkspace, listener_id: ListenerId) -> AppResult<&ProxyListener> {
    workspace
        .listeners
        .iter()
        .find(|listener| listener.id == listener_id)
        .ok_or_else(|| {
            AppError::new(
                "PROTOCOL_RULE_LISTENER_NOT_FOUND",
                "当前 Workspace 中不存在指定的入口。",
            )
            .entity(listener_id.to_string())
        })
}

fn ensure_requested_binding(
    input: &ProtocolRuleSaveInput,
    package: &ProtocolPackageRef,
    schema_version: u32,
) -> AppResult<()> {
    if input.package != *package {
        return Err(AppError::new(
            "PROTOCOL_RULE_PACKAGE_MISMATCH",
            "规则协议包必须与入口的精确绑定一致。",
        ));
    }
    if input.schema_version != schema_version {
        return Err(AppError::new(
            "PROTOCOL_RULE_SCHEMA_MISMATCH",
            "规则 Schema 版本必须与协议包编译描述一致。",
        ));
    }
    Ok(())
}

fn ensure_immutable_binding(
    current: &ProtocolDocumentRuleDefinition,
    input: &ProtocolRuleSaveInput,
) -> AppResult<()> {
    if current.listener_id() == input.listener_id
        && current.package() == &input.package
        && current.schema_version() == input.schema_version
        && current.stage() == input.stage
    {
        return Ok(());
    }
    Err(AppError::new(
        "PROTOCOL_RULE_BINDING_IMMUTABLE",
        "更新规则不能改变入口、协议包、Schema 或处理阶段绑定。",
    )
    .entity(current.rule_id().to_string()))
}

fn next_created_order(
    rules: &[ProtocolDocumentRuleDefinition],
    persisted_high_water: u64,
) -> AppResult<u64> {
    rules
        .iter()
        .map(ProtocolDocumentRuleDefinition::created_order)
        .max()
        .unwrap_or(0)
        .max(persisted_high_water)
        .checked_add(1)
        .filter(|value| (1..=MAX_JAVASCRIPT_SAFE_INTEGER).contains(value))
        .ok_or_else(|| {
            AppError::new(
                "PROTOCOL_RULE_CREATED_ORDER_EXHAUSTED",
                "协议报文规则创建顺序已达到可表示上限。",
            )
        })
}

fn protocol_rule_not_found(rule_id: ProtocolDocumentRuleId) -> AppError {
    AppError::new(
        "PROTOCOL_RULE_NOT_FOUND",
        "当前 Workspace 中不存在指定的 协议报文规则。",
    )
    .entity(rule_id.to_string())
}
