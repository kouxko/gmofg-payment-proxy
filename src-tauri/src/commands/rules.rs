//! 故障注入命令适配层。

use intercept_proxy_application::{
    ActiveFaultViewModel, FaultConfigurationDraft, FaultTemplateViewModel, RuleId,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

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
