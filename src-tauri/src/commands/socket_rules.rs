//! Socket Document 规则命令的薄适配层。

use intercept_proxy_application::{
    DocumentValue, ListenerId, OperationResultViewModel, ProtocolPackageSchemaFieldTypeViewModel,
    SocketDocumentRuleDefinition, SocketDocumentRuleId, SocketRuleCapabilityCatalog,
    SocketRuleSaveInput, SocketRuleStage, parse_socket_rule_value,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

/// 把规则编辑器文本解析为 Rust/Schema 认可的类型化值。
///
/// 该纯命令不依赖当前 Workspace；前端切换字段时只需提交公开字段类型，不能自行解释
/// UTF-8 字节、JavaScript 安全整数或 Blob Hex。
#[tauri::command]
#[specta::specta]
// Tauri IPC 必须拥有反序列化后的 String；CommandResult 则沿用全应用稳定的错误 DTO。
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn socket_rule_parse_value(
    field_type: ProtocolPackageSchemaFieldTypeViewModel,
    raw: String,
) -> CommandResult<DocumentValue> {
    parse_socket_rule_value(field_type, &raw).map_err(command_error)
}

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
    stage: SocketRuleStage,
) -> CommandResult<SocketRuleCapabilityCatalog> {
    state
        .application
        .socket_rule_capabilities(listener_id, stage)
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

#[cfg(test)]
#[path = "socket_rules/tests.rs"]
mod tests;
