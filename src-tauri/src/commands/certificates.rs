//! 证书命令适配层：文件选择、密钥材料处理与校验由 application 门面负责，本层不接触证书内容。

use intercept_proxy_application::{
    CertificateOverviewViewModel, CertificateValidationViewModel, OperationResultViewModel,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn certificate_overview(
    state: State<'_, AppState>,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_overview()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_generate_ca(
    state: State<'_, AppState>,
    sans: Vec<String>,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_generate_ca(sans)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_export_ca(
    state: State<'_, AppState>,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .certificate_export_ca()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_reissue_leaf(
    state: State<'_, AppState>,
    expected_revision: u64,
    sans: Vec<String>,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_reissue_leaf(expected_revision, sans)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_import_pkcs12(
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_import_pkcs12(password)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_import_upstream_ca(
    state: State<'_, AppState>,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_import_upstream_ca()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_validate(
    state: State<'_, AppState>,
) -> CommandResult<CertificateValidationViewModel> {
    state
        .application
        .certificate_validate()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_reset_ca(
    state: State<'_, AppState>,
    expected_revision: u64,
    confirmed: bool,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_reset_ca(expected_revision, confirmed)
        .await
        .map_err(command_error)
}
