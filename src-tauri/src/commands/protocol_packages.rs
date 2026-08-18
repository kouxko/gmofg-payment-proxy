//! 协议包管理 IPC。
//!
//! 命令只接收精确 `id + version` 身份；导入命令甚至不接收路径或字节。ZIP 文件选择、
//! 有界读取、完整编译、引用约束和启停约束都在 Rust Application/Infrastructure 中执行。

use intercept_proxy_application::{
    AppError, ListenerProtocolPackageCatalogViewModel, OperationResultViewModel,
    ProtocolPackageDetailViewModel, ProtocolPackageExportOutcomeViewModel,
    ProtocolPackageGroupViewModel, ProtocolPackageImportPreviewViewModel,
    ProtocolPackageImportToken, ProtocolPackageImportViewModel, ProtocolPackageRef,
    ProtocolPackageUsageViewModel, ProtocolPackageVersionViewModel,
};
use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageVersion};
use intercept_proxy_infrastructure::{AtomicFileExporter, InfrastructureError};
use serde::Deserialize;
use specta::Type;
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

const PROTOCOL_PACKAGE_EXPORT_DIALOG_PURPOSE: &str = "protocol_package_export_zip";
const BUILTIN_PROTOCOL_PACKAGE_FILE_NAME: &str = "iso8583-ascii-standard-1.0.0.zip";

/// `WebView` 提交的未验证协议包身份。
/// IPC 先反序列化普通字符串，再在命令内部调用领域构造器，因此恶意或过期前端提交的
/// 非法 ID/SemVer 也会得到稳定 `PROTOCOL_PACKAGE_INVALID`，而不是框架私有反序列化文本。
#[derive(Debug, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ProtocolPackageIdentityInput {
    pub id: String,
    pub version: String,
}

impl TryFrom<ProtocolPackageIdentityInput> for ProtocolPackageRef {
    type Error = AppError;

    fn try_from(value: ProtocolPackageIdentityInput) -> Result<Self, Self::Error> {
        Ok(Self {
            id: ProtocolPackageId::new(value.id)?,
            version: ProtocolPackageVersion::new(value.version)?,
        })
    }
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<ProtocolPackageGroupViewModel>> {
    state
        .application
        .protocol_package_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_protocol_package_catalog(
    state: State<'_, AppState>,
) -> CommandResult<ListenerProtocolPackageCatalogViewModel> {
    state
        .application
        .listener_protocol_package_catalog()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_detail(
    state: State<'_, AppState>,
    package_ref: ProtocolPackageIdentityInput,
) -> CommandResult<ProtocolPackageDetailViewModel> {
    let package_ref = package_ref.try_into().map_err(command_error)?;
    state
        .application
        .protocol_package_detail(package_ref)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_import(
    state: State<'_, AppState>,
) -> CommandResult<Option<ProtocolPackageImportPreviewViewModel>> {
    state
        .application
        .protocol_package_import()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_import_commit(
    state: State<'_, AppState>,
    token: ProtocolPackageImportToken,
) -> CommandResult<ProtocolPackageImportViewModel> {
    state
        .application
        .protocol_package_import_commit(token)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_import_discard(
    state: State<'_, AppState>,
    token: ProtocolPackageImportToken,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .protocol_package_import_discard(token)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_restore_builtin(
    state: State<'_, AppState>,
) -> CommandResult<ProtocolPackageImportViewModel> {
    state
        .application
        .protocol_package_restore_builtin()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_export_builtin(
    state: State<'_, AppState>,
) -> CommandResult<Option<ProtocolPackageExportOutcomeViewModel>> {
    let host = state.host();
    let dialog = host.file_dialog();
    let selection = tokio::task::spawn_blocking(move || {
        dialog.choose_save_file(
            PROTOCOL_PACKAGE_EXPORT_DIALOG_PURPOSE,
            BUILTIN_PROTOCOL_PACKAGE_FILE_NAME,
        )
    })
    .await
    .map_err(|_| command_error(export_task_failed()))?
    .map_err(command_error)?;
    let Some(selection) = selection else {
        return Ok(None);
    };

    let archive = state
        .application
        .protocol_package_builtin_archive()
        .await
        .map_err(command_error)?;
    let outcome = tokio::task::spawn_blocking(move || {
        AtomicFileExporter.write(&selection.path, &archive, selection.overwrite_confirmed)
    })
    .await
    .map_err(|_| command_error(export_task_failed()))?
    .map_err(|error| command_error(export_failed(&error)))?;

    Ok(Some(ProtocolPackageExportOutcomeViewModel {
        bytes_written: outcome.bytes_written,
        replaced_existing: outcome.replaced_existing,
    }))
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_enable(
    state: State<'_, AppState>,
    package_ref: ProtocolPackageIdentityInput,
) -> CommandResult<ProtocolPackageVersionViewModel> {
    let package_ref = package_ref.try_into().map_err(command_error)?;
    state
        .application
        .protocol_package_enable(package_ref)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_disable(
    state: State<'_, AppState>,
    package_ref: ProtocolPackageIdentityInput,
) -> CommandResult<ProtocolPackageVersionViewModel> {
    let package_ref = package_ref.try_into().map_err(command_error)?;
    state
        .application
        .protocol_package_disable(package_ref)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_delete(
    state: State<'_, AppState>,
    package_ref: ProtocolPackageIdentityInput,
) -> CommandResult<OperationResultViewModel> {
    let package_ref = package_ref.try_into().map_err(command_error)?;
    state
        .application
        .protocol_package_delete(package_ref)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn protocol_package_usage(
    state: State<'_, AppState>,
    package_ref: ProtocolPackageIdentityInput,
) -> CommandResult<Vec<ProtocolPackageUsageViewModel>> {
    let package_ref = package_ref.try_into().map_err(command_error)?;
    state
        .application
        .protocol_package_usage(package_ref)
        .await
        .map_err(command_error)
}

fn export_task_failed() -> AppError {
    AppError::new(
        "PROTOCOL_PACKAGE_EXPORT_FAILED",
        "协议包 ZIP 后台写入任务未能完成。",
    )
}

fn export_failed(error: &InfrastructureError) -> AppError {
    match error {
        InfrastructureError::ExportTargetExists { .. } => AppError::new(
            "PROTOCOL_PACKAGE_EXPORT_TARGET_EXISTS",
            "目标文件已存在，未执行覆盖。",
        ),
        InfrastructureError::ExportParentSync { .. } => AppError::new(
            "PROTOCOL_PACKAGE_EXPORT_DURABILITY_UNCERTAIN",
            "目标文件已替换，但目录持久化状态无法确认。",
        ),
        _ => AppError::new(
            "PROTOCOL_PACKAGE_EXPORT_FAILED",
            "协议包 ZIP 写入失败，原目标未被修改。",
        ),
    }
}

#[cfg(test)]
include!("protocol_packages/tests.rs");
