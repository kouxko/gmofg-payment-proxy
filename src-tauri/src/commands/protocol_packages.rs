//! 协议包管理 IPC。
//!
//! 命令只接收精确 `id + version` 身份；导入命令甚至不接收路径或字节。ZIP 文件选择、
//! 有界读取、完整编译、引用约束和启停约束都在 Rust Application/Infrastructure 中执行。

use intercept_proxy_application::{
    AppError, ListenerProtocolPackageCatalogViewModel, OperationResultViewModel,
    ProtocolPackageDetailViewModel, ProtocolPackageGroupViewModel,
    ProtocolPackageImportPreviewViewModel, ProtocolPackageImportToken,
    ProtocolPackageImportViewModel, ProtocolPackageRef, ProtocolPackageUsageViewModel,
    ProtocolPackageVersionViewModel,
};
use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageVersion};
use serde::Deserialize;
use specta::Type;
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

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

#[cfg(test)]
include!("protocol_packages/tests.rs");
