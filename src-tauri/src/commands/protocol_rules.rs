//! 统一规则命令的薄适配层。

use intercept_proxy_application::{
    DocumentValue, MessageStage, OperationResultViewModel, ProtocolPackageSchemaFieldTypeViewModel,
    ProtocolRuleCommonActionCapability, RuleActionKind, RuleConditionKind, RuleDefinition,
    RuleDefinitionSaveInput, RuleEditorContext, RuleLocalDocumentActionKind,
    RuleLocalDocumentPredicateKind, RuleLocalDocumentValueType, parse_protocol_rule_value,
};
use intercept_proxy_domain::{Condition, HttpAction, ListenerId, Revision, RuleId, UnifiedAction};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn rule_parse_document_value(
    field_type: ProtocolPackageSchemaFieldTypeViewModel,
    raw: String,
) -> CommandResult<DocumentValue> {
    parse_protocol_rule_value(field_type, &raw).map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn rule_definition_condition_draft(
    app_state: State<'_, AppState>,
    kind: RuleConditionKind,
    stage: MessageStage,
) -> CommandResult<Condition> {
    app_state
        .application
        .rule_definition_condition_draft(kind, stage)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn rule_definition_action_draft(
    app_state: State<'_, AppState>,
    kind: RuleActionKind,
    stage: MessageStage,
) -> CommandResult<HttpAction> {
    app_state
        .application
        .rule_definition_action_draft(kind, stage)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn rule_definition_document_condition_draft(
    app_state: State<'_, AppState>,
    path: String,
    value_type: RuleLocalDocumentValueType,
    predicate: RuleLocalDocumentPredicateKind,
    raw: String,
) -> CommandResult<Condition> {
    app_state
        .application
        .rule_definition_document_condition_draft(&path, value_type, predicate, &raw)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn rule_definition_document_action_draft(
    app_state: State<'_, AppState>,
    path: String,
    value_type: RuleLocalDocumentValueType,
    action: RuleLocalDocumentActionKind,
    raw: Option<String>,
    index: Option<u32>,
) -> CommandResult<UnifiedAction> {
    app_state
        .application
        .rule_definition_document_action_draft(&path, value_type, action, raw.as_deref(), index)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(
    clippy::needless_pass_by_value,
    clippy::result_large_err,
    clippy::unnecessary_wraps
)]
pub fn rule_definition_document_common_action_draft(
    app_state: State<'_, AppState>,
    action: ProtocolRuleCommonActionCapability,
) -> CommandResult<UnifiedAction> {
    Ok(app_state
        .application
        .rule_definition_document_common_action_draft(action))
}

#[tauri::command]
#[specta::specta]
pub async fn rule_definition_list(
    app_state: State<'_, AppState>,
) -> CommandResult<Vec<RuleDefinition>> {
    app_state
        .application
        .rule_definition_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_editor_context(
    app_state: State<'_, AppState>,
    listener_id: ListenerId,
) -> CommandResult<RuleEditorContext> {
    app_state
        .application
        .rule_editor_context(listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_definition_get(
    app_state: State<'_, AppState>,
    rule_id: RuleId,
) -> CommandResult<RuleDefinition> {
    app_state
        .application
        .rule_definition_get(rule_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_definition_copy(
    app_state: State<'_, AppState>,
    rule_id: RuleId,
) -> CommandResult<RuleDefinition> {
    app_state
        .application
        .rule_definition_copy(rule_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_definition_create_from_exchange_observation(
    app_state: State<'_, AppState>,
    exchange_id: String,
    response_event_index: u32,
) -> CommandResult<RuleDefinitionSaveInput> {
    let record = app_state
        .exchange_observations()
        .get(&exchange_id)
        .ok_or_else(|| {
            command_error(intercept_proxy_application::AppError::new(
                "EXCHANGE_OBSERVATION_NOT_FOUND",
                "Exchange 运行记录不存在或已被内存淘汰。",
            ))
        })?;
    app_state
        .application
        .rule_definition_create_from_exchange_observation(&record, response_event_index as usize)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_definition_save(
    app_state: State<'_, AppState>,
    input: RuleDefinitionSaveInput,
) -> CommandResult<RuleDefinition> {
    app_state
        .application
        .rule_definition_save(input)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_definition_toggle(
    app_state: State<'_, AppState>,
    rule_id: RuleId,
    expected_revision: Revision,
    enabled: bool,
) -> CommandResult<RuleDefinition> {
    app_state
        .application
        .rule_definition_toggle(rule_id, expected_revision, enabled)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_definition_delete(
    app_state: State<'_, AppState>,
    rule_id: RuleId,
    expected_revision: Revision,
    confirmed: bool,
) -> CommandResult<OperationResultViewModel> {
    app_state
        .application
        .rule_definition_delete(rule_id, expected_revision, confirmed)
        .await
        .map_err(command_error)
}
