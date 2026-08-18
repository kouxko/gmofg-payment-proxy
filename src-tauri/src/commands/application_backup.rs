//! Application backup ZIP IPC adapter.

use intercept_proxy_application::{
    ApplicationBackupExportOutcome, ApplicationBackupImportCommitOutcome,
    ApplicationBackupImportPreview, ApplicationBackupImportToken, OperationResultViewModel,
};
use intercept_proxy_infrastructure::{
    ApplicationBackupFileExporter, AtomicFileExporter, DEFAULT_MAX_APPLICATION_BACKUP_ARCHIVE_BYTES,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

const APPLICATION_BACKUP_DIALOG_PURPOSE: &str = "application_backup_zip";
const APPLICATION_BACKUP_FILE_NAME: &str = "intercept-proxy-backup.zip";

#[tauri::command]
#[specta::specta]
pub async fn application_backup_export(
    state: State<'_, AppState>,
) -> CommandResult<Option<ApplicationBackupExportOutcome>> {
    let host = state.host();
    let dialog = host.file_dialog();
    let selection = tokio::task::spawn_blocking(move || {
        dialog.choose_save_file(
            APPLICATION_BACKUP_DIALOG_PURPOSE,
            APPLICATION_BACKUP_FILE_NAME,
        )
    })
    .await
    .map_err(|_| command_error(task_failed()))?
    .map_err(command_error)?;
    let Some(selection) = selection else {
        return Ok(None);
    };
    let exporter =
        ApplicationBackupFileExporter::new(selection.path, selection.overwrite_confirmed);
    state
        .application
        .application_backup_export(&exporter)
        .await
        .map(Some)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn application_backup_import_prepare(
    state: State<'_, AppState>,
) -> CommandResult<Option<ApplicationBackupImportPreview>> {
    let host = state.host();
    let dialog = host.file_dialog();
    let path = tokio::task::spawn_blocking(move || {
        dialog.choose_open_file(APPLICATION_BACKUP_DIALOG_PURPOSE)
    })
    .await
    .map_err(|_| command_error(task_failed()))?
    .map_err(command_error)?;
    let Some(path) = path else {
        return Ok(None);
    };
    let bytes = tokio::task::spawn_blocking(move || {
        AtomicFileExporter.read_bounded(&path, DEFAULT_MAX_APPLICATION_BACKUP_ARCHIVE_BYTES)
    })
    .await
    .map_err(|_| command_error(task_failed()))?
    .map_err(|_| {
        command_error(intercept_proxy_application::AppError::new(
            "APPLICATION_BACKUP_FILE_READ_FAILED",
            "无法读取所选应用备份文件。",
        ))
    })?;
    let importer = host.application_backup_importer();
    state
        .application
        .application_backup_import_prepare(importer.as_ref(), bytes)
        .await
        .map(Some)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn application_backup_import_commit(
    state: State<'_, AppState>,
    token: ApplicationBackupImportToken,
) -> CommandResult<ApplicationBackupImportCommitOutcome> {
    let importer = state.host().application_backup_importer();
    state
        .application
        .application_backup_import_commit(importer.as_ref(), token)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn application_backup_import_discard(
    state: State<'_, AppState>,
    token: ApplicationBackupImportToken,
) -> CommandResult<OperationResultViewModel> {
    let importer = state.host().application_backup_importer();
    state
        .application
        .application_backup_import_discard(importer.as_ref(), token)
        .await
        .map(|()| OperationResultViewModel::success("应用备份预览已丢弃。"))
        .map_err(command_error)
}

fn task_failed() -> intercept_proxy_application::AppError {
    intercept_proxy_application::AppError::new(
        "APPLICATION_BACKUP_FILE_TASK_FAILED",
        "应用备份文件操作未能完成。",
    )
}
