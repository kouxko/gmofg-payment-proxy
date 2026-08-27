//! 规则与故障注入命令适配层：草稿解析、版本检查和运行时操作全部委托给 application 门面。

use intercept_proxy_application::{
    ActiveFaultViewModel, FaultConfigurationDraft, FaultTemplateViewModel, MessageStage,
    OperationResultViewModel, RuleAction, RuleActionKind, RuleByteInputViewModel, RuleCondition,
    RuleConditionKind, RuleDraft, RuleHeaderInputViewModel, RuleId, RuleMatchField,
    RuleMatchFieldKind, RuleMatchOperator, RuleMatchOperatorKind, RuleStageCapabilityViewModel,
    RuleSummaryViewModel, RuleViewModel, SessionId,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn rule_list(state: State<'_, AppState>) -> CommandResult<Vec<RuleSummaryViewModel>> {
    state.application.rule_list().await.map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_get(state: State<'_, AppState>, rule_id: RuleId) -> CommandResult<RuleViewModel> {
    state
        .application
        .rule_get(rule_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_new_draft(state: State<'_, AppState>) -> CommandResult<RuleDraft> {
    state
        .application
        .rule_new_draft()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_capabilities(
    state: State<'_, AppState>,
) -> CommandResult<Vec<RuleStageCapabilityViewModel>> {
    Ok(state.application.rule_capabilities())
}

#[tauri::command]
#[specta::specta]
pub async fn rule_condition_draft(
    app_state: State<'_, AppState>,
    kind: RuleConditionKind,
    stage: MessageStage,
) -> CommandResult<RuleCondition> {
    app_state
        .application
        .rule_condition_draft(kind, stage)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_action_draft(
    app_state: State<'_, AppState>,
    kind: RuleActionKind,
    stage: MessageStage,
) -> CommandResult<RuleAction> {
    app_state
        .application
        .rule_action_draft(kind, stage)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_match_field_draft(
    app_state: State<'_, AppState>,
    kind: RuleMatchFieldKind,
    stage: MessageStage,
) -> CommandResult<RuleMatchField> {
    app_state
        .application
        .rule_match_field_draft(kind, stage)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_match_operator_draft(
    state: State<'_, AppState>,
    kind: RuleMatchOperatorKind,
) -> CommandResult<RuleMatchOperator> {
    Ok(state.application.rule_match_operator_draft(kind))
}

#[tauri::command]
#[specta::specta]
pub async fn rule_parse_byte_input(
    state: State<'_, AppState>,
    raw: String,
) -> CommandResult<RuleByteInputViewModel> {
    state
        .application
        .rule_parse_byte_input(&raw)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_parse_header_input(
    state: State<'_, AppState>,
    raw: String,
) -> CommandResult<RuleHeaderInputViewModel> {
    state
        .application
        .rule_parse_header_input(&raw)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_create_from_session(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<RuleDraft> {
    state
        .application
        .rule_create_from_session(session_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_create_from_exchange_observation(
    state: State<'_, AppState>,
    exchange_id: String,
    response_event_index: u32,
) -> CommandResult<RuleDraft> {
    let record = state
        .exchange_observations()
        .get(&exchange_id)
        .ok_or_else(|| {
            command_error(intercept_proxy_application::AppError::new(
                "EXCHANGE_OBSERVATION_NOT_FOUND",
                "Exchange 运行记录不存在或已被内存淘汰。",
            ))
        })?;
    state
        .application
        .rule_create_from_exchange_observation(&record, response_event_index as usize)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_save(
    state: State<'_, AppState>,
    draft: RuleDraft,
) -> CommandResult<RuleViewModel> {
    state
        .application
        .rule_save(draft)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_copy(
    state: State<'_, AppState>,
    rule_id: RuleId,
) -> CommandResult<RuleViewModel> {
    state
        .application
        .rule_copy(rule_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_delete(
    state: State<'_, AppState>,
    rule_id: RuleId,
    expected_revision: u64,
    confirmed: bool,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .rule_delete(rule_id, expected_revision, confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_toggle(
    state: State<'_, AppState>,
    rule_id: RuleId,
    expected_revision: u64,
    enabled: bool,
) -> CommandResult<RuleViewModel> {
    state
        .application
        .rule_toggle(rule_id, expected_revision, enabled)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_import(state: State<'_, AppState>) -> CommandResult<OperationResultViewModel> {
    state.application.rule_import().await.map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_export(state: State<'_, AppState>) -> CommandResult<OperationResultViewModel> {
    state.application.rule_export().await.map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn fault_template_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<FaultTemplateViewModel>> {
    state
        .application
        .fault_template_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn fault_configure(
    state: State<'_, AppState>,
    draft: FaultConfigurationDraft,
) -> CommandResult<ActiveFaultViewModel> {
    state
        .application
        .fault_configure(draft)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn fault_active_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<ActiveFaultViewModel>> {
    state
        .application
        .fault_active_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn fault_stop(
    state: State<'_, AppState>,
    rule_id: RuleId,
    expected_revision: u64,
    confirmed: bool,
) -> CommandResult<ActiveFaultViewModel> {
    state
        .application
        .fault_stop(rule_id, expected_revision, confirmed)
        .await
        .map_err(command_error)
}
