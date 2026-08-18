//! Application backup ZIP IPC adapter.

use intercept_proxy_application::{
    ApplicationBackupExportOutcome, ApplicationBackupImportCommitOutcome,
    ApplicationBackupImportPreview, ApplicationBackupImportToken, LegacyImportPreview,
    LegacyImportToken, MAX_APPLICATION_CONFIGURATION_BYTES, MAX_WORKSPACE_DOCUMENT_BYTES,
    OperationResultViewModel,
};
use intercept_proxy_infrastructure::{
    ApplicationBackupFileExporter, AtomicFileExporter,
    DEFAULT_MAX_APPLICATION_BACKUP_ARCHIVE_BYTES, NativeFileDialog,
};
use std::sync::Arc;
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

const APPLICATION_BACKUP_DIALOG_PURPOSE: &str = "application_backup_zip";
const APPLICATION_BACKUP_FILE_NAME: &str = "intercept-proxy-backup.zip";
const LEGACY_CONFIGURATION_DIALOG_PURPOSE: &str = "intercept_configuration";
const LEGACY_WORKSPACE_DIALOG_PURPOSE: &str = "intercept_workspace";

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

#[tauri::command]
#[specta::specta]
pub async fn legacy_application_configuration_import_prepare(
    state: State<'_, AppState>,
) -> CommandResult<Option<LegacyImportPreview>> {
    let Some(bytes) = pick_bounded_file(
        state.host().file_dialog(),
        LEGACY_CONFIGURATION_DIALOG_PURPOSE,
        MAX_APPLICATION_CONFIGURATION_BYTES as u64,
    )
    .await?
    else {
        return Ok(None);
    };
    let pending = state.host().legacy_importer();
    state
        .application
        .legacy_application_configuration_import_prepare(pending.as_ref(), bytes)
        .await
        .map(Some)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn legacy_application_configuration_import_commit(
    state: State<'_, AppState>,
    token: LegacyImportToken,
) -> CommandResult<OperationResultViewModel> {
    let pending = state.host().legacy_importer();
    state
        .application
        .legacy_application_configuration_import_commit(pending.as_ref(), token)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn legacy_application_configuration_import_discard(
    state: State<'_, AppState>,
    token: LegacyImportToken,
) -> CommandResult<OperationResultViewModel> {
    discard_legacy(state, token).await
}

#[tauri::command]
#[specta::specta]
pub async fn legacy_workspace_import_prepare(
    state: State<'_, AppState>,
) -> CommandResult<Option<LegacyImportPreview>> {
    let Some(bytes) = pick_bounded_file(
        state.host().file_dialog(),
        LEGACY_WORKSPACE_DIALOG_PURPOSE,
        MAX_WORKSPACE_DOCUMENT_BYTES as u64,
    )
    .await?
    else {
        return Ok(None);
    };
    let pending = state.host().legacy_importer();
    state
        .application
        .legacy_workspace_import_prepare(pending.as_ref(), bytes)
        .await
        .map(Some)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn legacy_workspace_import_commit(
    state: State<'_, AppState>,
    token: LegacyImportToken,
) -> CommandResult<OperationResultViewModel> {
    let pending = state.host().legacy_importer();
    state
        .application
        .legacy_workspace_import_commit(pending.as_ref(), token)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn legacy_workspace_import_discard(
    state: State<'_, AppState>,
    token: LegacyImportToken,
) -> CommandResult<OperationResultViewModel> {
    discard_legacy(state, token).await
}

async fn discard_legacy(
    state: State<'_, AppState>,
    token: LegacyImportToken,
) -> CommandResult<OperationResultViewModel> {
    let pending = state.host().legacy_importer();
    state
        .application
        .legacy_import_discard(pending.as_ref(), token)
        .await
        .map(|()| OperationResultViewModel::success("旧版导入预览已丢弃。"))
        .map_err(command_error)
}

async fn pick_bounded_file(
    dialog: Arc<dyn NativeFileDialog>,
    purpose: &'static str,
    max_bytes: u64,
) -> CommandResult<Option<Vec<u8>>> {
    let path = tokio::task::spawn_blocking(move || dialog.choose_open_file(purpose))
        .await
        .map_err(|_| command_error(task_failed()))?
        .map_err(command_error)?;
    let Some(path) = path else {
        return Ok(None);
    };
    tokio::task::spawn_blocking(move || AtomicFileExporter.read_bounded(&path, max_bytes))
        .await
        .map_err(|_| command_error(task_failed()))?
        .map(Some)
        .map_err(|_| {
            command_error(intercept_proxy_application::AppError::new(
                "LEGACY_IMPORT_FILE_READ_FAILED",
                "无法读取所选旧版配置文件。",
            ))
        })
}

fn task_failed() -> intercept_proxy_application::AppError {
    intercept_proxy_application::AppError::new(
        "APPLICATION_BACKUP_FILE_TASK_FAILED",
        "应用备份文件操作未能完成。",
    )
}
