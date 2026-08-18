//! 工作区命令适配层：保持 IPC 契约，持久化与校验仍由 application 门面负责。

use intercept_proxy_application::{
    OperationResultViewModel, ProxyWorkspace, SecretReference, WorkspaceId,
    WorkspaceSummaryViewModel, WorkspaceValidationViewModel,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn workspace_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<WorkspaceSummaryViewModel>> {
    state
        .application
        .workspace_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_get(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_get(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
// Tauri passes deserialized command values by ownership; changing these to references would make
// the generated IPC command incompatible with its extractor.
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn workspace_component_new(
    state: State<'_, AppState>,
    workspace: ProxyWorkspace,
    kind: String,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_component_new(workspace, &kind)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn workspace_component_apply_intent(
    state: State<'_, AppState>,
    workspace: ProxyWorkspace,
    component_kind: String,
    component_id: String,
    operation: String,
    value: String,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_component_apply_intent(
            workspace,
            &component_kind,
            &component_id,
            &operation,
            &value,
        )
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_secret_store_basic(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> CommandResult<SecretReference> {
    state
        .application
        .workspace_secret_store_basic(username, password)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_create(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_create(name)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_copy(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_copy(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_select(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<WorkspaceSummaryViewModel> {
    state
        .application
        .workspace_select(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_validate(
    state: State<'_, AppState>,
    workspace: ProxyWorkspace,
) -> CommandResult<WorkspaceValidationViewModel> {
    state
        .application
        .workspace_validate(workspace)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_save(
    state: State<'_, AppState>,
    workspace: ProxyWorkspace,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_save(workspace)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_delete(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_revision: u64,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .workspace_delete(workspace_id, expected_revision)
        .await
        .map_err(command_error)
}
