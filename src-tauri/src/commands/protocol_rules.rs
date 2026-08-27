//! 协议 Document 规则命令的薄适配层。

use intercept_proxy_application::{
    DocumentValue, ListenerId, OperationResultViewModel, ProtocolDocumentRuleDefinition,
    ProtocolDocumentRuleId, ProtocolPackageSchemaFieldTypeViewModel, ProtocolRuleCapabilityCatalog,
    ProtocolRuleEditorContext, ProtocolRuleSaveInput, ProtocolRuleStage, parse_protocol_rule_value,
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
pub fn protocol_rule_parse_value(
    field_type: ProtocolPackageSchemaFieldTypeViewModel,
    raw: String,
) -> CommandResult<DocumentValue> {
    parse_protocol_rule_value(field_type, &raw).map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_rule_list(
    app_state: State<'_, AppState>,
) -> CommandResult<Vec<ProtocolDocumentRuleDefinition>> {
    app_state
        .application
        .protocol_rule_list()
        .await
        .map_err(command_error)
}

/// 保留给非编辑器调用方的单阶段只读查询；WebView 编辑器必须使用完整上下文命令。
#[tauri::command]
#[specta::specta]
pub async fn protocol_rule_capabilities(
    app_state: State<'_, AppState>,
    listener_id: ListenerId,
    stage: ProtocolRuleStage,
) -> CommandResult<ProtocolRuleCapabilityCatalog> {
    app_state
        .application
        .protocol_rule_capabilities(listener_id, stage)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_rule_editor_context(
    app_state: State<'_, AppState>,
    listener_id: ListenerId,
) -> CommandResult<ProtocolRuleEditorContext> {
    app_state
        .application
        .protocol_rule_editor_context(listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_rule_save(
    app_state: State<'_, AppState>,
    input: ProtocolRuleSaveInput,
) -> CommandResult<ProtocolDocumentRuleDefinition> {
    app_state
        .application
        .protocol_rule_save(input)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_rule_toggle(
    app_state: State<'_, AppState>,
    rule_id: ProtocolDocumentRuleId,
    expected_revision: u64,
    enabled: bool,
) -> CommandResult<ProtocolDocumentRuleDefinition> {
    app_state
        .application
        .protocol_rule_toggle(rule_id, expected_revision, enabled)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_rule_delete(
    app_state: State<'_, AppState>,
    rule_id: ProtocolDocumentRuleId,
    expected_revision: u64,
    confirmed: bool,
) -> CommandResult<OperationResultViewModel> {
    app_state
        .application
        .protocol_rule_delete(rule_id, expected_revision, confirmed)
        .await
        .map_err(command_error)
}

#[cfg(test)]
#[path = "protocol_rules/tests.rs"]
mod tests;
