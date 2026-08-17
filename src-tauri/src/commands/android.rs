//! Android 设备与弱网命令适配层：设备操作由 application 门面实现，本层不持有运行时策略。

use intercept_proxy_application::{
    AndroidAdbViewModel, AndroidCompanionInstallViewModel, AndroidDeviceViewModel,
    AndroidNetworkProfile, AndroidNetworkProfileSummary, AndroidNetworkStatusViewModel,
    AndroidPackageViewModel, AndroidProfileEditIntent, AndroidRuntimeOwnerViewModel,
    OperationResultViewModel,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn android_adb_get(state: State<'_, AppState>) -> CommandResult<AndroidAdbViewModel> {
    state
        .application
        .android_adb_get()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_adb_select(
    state: State<'_, AppState>,
    serial: String,
) -> CommandResult<AndroidAdbViewModel> {
    state
        .application
        .android_adb_select(serial)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_device_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AndroidDeviceViewModel>> {
    state
        .application
        .android_device_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_package_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AndroidPackageViewModel>> {
    state
        .application
        .android_package_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_package_refresh(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AndroidPackageViewModel>> {
    state
        .application
        .android_package_refresh()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_package_query(
    state: State<'_, AppState>,
    query: String,
) -> CommandResult<Vec<AndroidPackageViewModel>> {
    state
        .application
        .android_package_query(query)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_package_get(
    state: State<'_, AppState>,
    package_name: String,
) -> CommandResult<AndroidPackageViewModel> {
    state
        .application
        .android_package_get(package_name)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_companion_install(
    state: State<'_, AppState>,
) -> CommandResult<AndroidCompanionInstallViewModel> {
    state
        .application
        .android_companion_install()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_companion_update(
    state: State<'_, AppState>,
) -> CommandResult<AndroidCompanionInstallViewModel> {
    state
        .application
        .android_companion_update()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_vpn_open_consent(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .android_vpn_open_consent()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AndroidNetworkProfileSummary>> {
    state
        .application
        .device_network_profile_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
// Tauri owns extracted State/JSON arguments, and the IPC contract intentionally keeps the same
// structured error envelope even for a Rust-owned default constructor.
#[allow(
    clippy::needless_pass_by_value,
    clippy::result_large_err,
    clippy::unnecessary_wraps
)]
pub fn device_network_profile_new(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkProfile> {
    Ok(state.application.device_network_profile_new())
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_get(
    state: State<'_, AppState>,
    profile_id: String,
) -> CommandResult<AndroidNetworkProfile> {
    state
        .application
        .device_network_profile_get(profile_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_apply_intent(
    state: State<'_, AppState>,
    profile: AndroidNetworkProfile,
    intent: AndroidProfileEditIntent,
) -> CommandResult<AndroidNetworkProfile> {
    state
        .application
        .device_network_profile_apply_intent(profile, intent)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_save(
    state: State<'_, AppState>,
    profile: AndroidNetworkProfile,
) -> CommandResult<AndroidNetworkProfile> {
    state
        .application
        .device_network_profile_save(profile)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_delete(
    state: State<'_, AppState>,
    profile_id: String,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .device_network_profile_delete(profile_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_start(
    state: State<'_, AppState>,
    profile_id: String,
    dangerous_confirmed: bool,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_start(profile_id, dangerous_confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_apply(
    state: State<'_, AppState>,
    profile_id: String,
    dangerous_confirmed: bool,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_apply(profile_id, dangerous_confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_stop(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_stop()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_emergency_restore(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_emergency_restore()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_status(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_status()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_runtime_owner(
    state: State<'_, AppState>,
) -> CommandResult<Option<AndroidRuntimeOwnerViewModel>> {
    state
        .application
        .device_network_runtime_owner()
        .await
        .map_err(command_error)
}
