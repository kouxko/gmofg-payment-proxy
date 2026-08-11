//! 监听器命令适配层：只转换 Tauri 参数和统一错误，不在此处实现证书或网络行为。

use intercept_proxy_application::{
    CertificateReference, ListenerCertificateDetailViewModel, ListenerCertificateImportViewModel,
    ListenerId, ListenerOverviewViewModel, ListenerStatusViewModel,
    ListenerUpstreamConnectionTestViewModel, ListenerUpstreamTlsTestViewModel,
    OperationResultViewModel, ProxyListener, ProxyWorkspace, WorkspaceId,
    WorkspaceValidationViewModel,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn listener_list(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<Vec<ProxyListener>> {
    state
        .application
        .listener_list(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn listener_new(state: State<'_, AppState>) -> CommandResult<ProxyListener> {
    state.application.listener_new().map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn listener_copy(
    state: State<'_, AppState>,
    source: ProxyListener,
) -> CommandResult<ProxyListener> {
    state
        .application
        .listener_copy(source)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_get(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    listener_id: ListenerId,
) -> CommandResult<ProxyListener> {
    state
        .application
        .listener_get(workspace_id, listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_save(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener: ProxyListener,
    certificate_references: Vec<CertificateReference>,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .listener_save(
            workspace_id,
            expected_workspace_revision,
            listener,
            certificate_references,
        )
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_validate(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener: ProxyListener,
    certificate_references: Vec<CertificateReference>,
) -> CommandResult<WorkspaceValidationViewModel> {
    state
        .application
        .listener_validate(
            workspace_id,
            expected_workspace_revision,
            listener,
            certificate_references,
        )
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_delete(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener_id: ListenerId,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .listener_delete(workspace_id, expected_workspace_revision, listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_statuses(
    state: State<'_, AppState>,
) -> CommandResult<Vec<ListenerStatusViewModel>> {
    state
        .application
        .listener_statuses()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_overview(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<ListenerOverviewViewModel> {
    state
        .application
        .listener_overview(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_start(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener_id: ListenerId,
) -> CommandResult<ListenerStatusViewModel> {
    state
        .application
        .listener_start(workspace_id, expected_workspace_revision, listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_stop(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener_id: ListenerId,
) -> CommandResult<ListenerStatusViewModel> {
    state
        .application
        .listener_stop(workspace_id, expected_workspace_revision, listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_test_upstream_tls(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener: ProxyListener,
    certificate_references: Vec<CertificateReference>,
) -> CommandResult<ListenerUpstreamTlsTestViewModel> {
    state
        .application
        .listener_test_upstream_tls(
            workspace_id,
            expected_workspace_revision,
            listener,
            certificate_references,
        )
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_test_upstream_connection(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener: ProxyListener,
    certificate_references: Vec<CertificateReference>,
) -> CommandResult<ListenerUpstreamConnectionTestViewModel> {
    state
        .application
        .listener_test_upstream_connection(
            workspace_id,
            expected_workspace_revision,
            listener,
            certificate_references,
        )
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_import_downstream_server_identity(
    state: State<'_, AppState>,
    label: String,
) -> CommandResult<Option<ListenerCertificateImportViewModel>> {
    state
        .application
        .listener_import_downstream_server_identity(label)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_import_downstream_client_trust(
    state: State<'_, AppState>,
    label: String,
) -> CommandResult<Option<ListenerCertificateImportViewModel>> {
    state
        .application
        .listener_import_downstream_client_trust(label)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_import_upstream_client_identity(
    state: State<'_, AppState>,
    label: String,
    password: String,
) -> CommandResult<Option<ListenerCertificateImportViewModel>> {
    state
        .application
        .listener_import_upstream_client_identity(label, password)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_import_upstream_server_trust(
    state: State<'_, AppState>,
    label: String,
) -> CommandResult<Option<ListenerCertificateImportViewModel>> {
    state
        .application
        .listener_import_upstream_server_trust(label)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_certificate_overview(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<Vec<ListenerCertificateDetailViewModel>> {
    state
        .application
        .listener_certificate_overview(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_certificate_discard(
    state: State<'_, AppState>,
    reference: CertificateReference,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .listener_certificate_discard(reference)
        .await
        .map_err(command_error)
}
