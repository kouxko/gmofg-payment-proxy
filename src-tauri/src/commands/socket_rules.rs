//! Socket Document 规则命令的薄适配层。

use intercept_proxy_application::{
    ListenerId, OperationResultViewModel, SocketDirection, SocketDocumentRuleDefinition,
    SocketDocumentRuleId, SocketRuleCapabilityCatalog, SocketRuleSaveInput,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn socket_rule_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<SocketDocumentRuleDefinition>> {
    state
        .application
        .socket_rule_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn socket_rule_capabilities(
    state: State<'_, AppState>,
    listener_id: ListenerId,
    direction: SocketDirection,
) -> CommandResult<SocketRuleCapabilityCatalog> {
    state
        .application
        .socket_rule_capabilities(listener_id, direction)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn socket_rule_save(
    state: State<'_, AppState>,
    input: SocketRuleSaveInput,
) -> CommandResult<SocketDocumentRuleDefinition> {
    state
        .application
        .socket_rule_save(input)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn socket_rule_toggle(
    state: State<'_, AppState>,
    rule_id: SocketDocumentRuleId,
    expected_revision: u64,
    enabled: bool,
) -> CommandResult<SocketDocumentRuleDefinition> {
    state
        .application
        .socket_rule_toggle(rule_id, expected_revision, enabled)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn socket_rule_delete(
    state: State<'_, AppState>,
    rule_id: SocketDocumentRuleId,
    expected_revision: u64,
    confirmed: bool,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .socket_rule_delete(rule_id, expected_revision, confirmed)
        .await
        .map_err(command_error)
}
