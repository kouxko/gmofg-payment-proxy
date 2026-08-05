//! 设置命令适配层：仅保留稳定的 Tauri 调用签名，默认值与校验规则由 application 门面拥有。

use intercept_proxy_application::{SettingsDraft, SettingsValidationViewModel, SettingsViewModel};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> CommandResult<SettingsViewModel> {
    state
        .application
        .settings_get()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_validate(
    state: State<'_, AppState>,
    draft: SettingsDraft,
) -> CommandResult<SettingsValidationViewModel> {
    state
        .application
        .settings_validate(draft)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_save(
    state: State<'_, AppState>,
    draft: SettingsDraft,
) -> CommandResult<SettingsViewModel> {
    state
        .application
        .settings_save(draft)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_reset_defaults(
    state: State<'_, AppState>,
    confirmed: bool,
) -> CommandResult<SettingsDraft> {
    state
        .application
        .settings_reset_defaults(confirmed)
        .await
        .map_err(command_error)
}
