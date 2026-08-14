//! 基础设施协议包注册表到 Application 生命周期端口的映射。
//!
//! 此处只转换无源码元数据与稳定错误；启停、引用和删除规则仍由 Application 用例决定。

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ProtocolPackageCompilationReceipt, ProtocolPackageCompilerPort,
    ProtocolPackageStorePort, ProtocolPackageValidationViewModel, ProtocolPackageVersionViewModel,
};

use super::{
    ProtocolPackageRef, ProtocolPackageRepositoryAdapter, ProtocolPackageStorageError,
    ProtocolPackageStorageErrorCode, ProtocolPackageSummary, ProtocolPackageValidationStatus,
};

#[async_trait]
impl ProtocolPackageStorePort for ProtocolPackageRepositoryAdapter {
    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        ProtocolPackageRepositoryAdapter::list(self)
            .map(|items| items.into_iter().map(application_summary).collect())
            .map_err(|error| protocol_package_app_error(&error))
    }

    async fn get(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        self.summary(package)
            .map(|summary| summary.map(application_summary))
            .map_err(|error| protocol_package_app_error(&error))
    }

    async fn set_enabled(&self, package: &ProtocolPackageRef, enabled: bool) -> AppResult<()> {
        ProtocolPackageRepositoryAdapter::set_enabled(self, package, enabled)
            .map_err(|error| protocol_package_app_error(&error))
    }

    async fn delete(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        ProtocolPackageRepositoryAdapter::delete(self, package)
            .map_err(|error| protocol_package_app_error(&error))
    }
}

#[async_trait]
impl ProtocolPackageCompilerPort for ProtocolPackageRepositoryAdapter {
    async fn validate_for_enable(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageCompilationReceipt> {
        let compiled = self
            .revalidate(package)
            .map_err(|error| protocol_package_app_error(&error))?;
        Ok(ProtocolPackageCompilationReceipt {
            package: compiled.package().clone(),
            host_api: compiled.manifest().api(),
            // `revalidate()` 走与导入相同的 Manifest parser；不受当前 Host 支持的 API 会在
            // 到达这里之前以稳定编译错误失败。
            compatible: true,
        })
    }
}

fn application_summary(summary: ProtocolPackageSummary) -> ProtocolPackageVersionViewModel {
    ProtocolPackageVersionViewModel {
        package: summary.package,
        name: summary.name,
        host_api: summary.host_api,
        enabled: summary.enabled,
        validation: match summary.validation {
            ProtocolPackageValidationStatus::Valid => ProtocolPackageValidationViewModel::Valid,
            ProtocolPackageValidationStatus::Invalid { code } => {
                ProtocolPackageValidationViewModel::Invalid { code }
            }
        },
        installed_at: summary.installed_at,
    }
}

pub(super) fn protocol_package_app_error(error: &ProtocolPackageStorageError) -> AppError {
    let entity = match error {
        ProtocolPackageStorageError::IdentityConflict { package }
        | ProtocolPackageStorageError::NotFound { package }
        | ProtocolPackageStorageError::StoredPackageInvalid { package, .. } => {
            Some(format!("{}@{}", package.id, package.version))
        }
        ProtocolPackageStorageError::Archive { .. }
        | ProtocolPackageStorageError::Compilation { .. }
        | ProtocolPackageStorageError::Infrastructure(_) => None,
    };
    let (code, message, retryable) = match error.code() {
        ProtocolPackageStorageErrorCode::ArchiveInvalid => (
            error
                .detail_code()
                .unwrap_or("PROTOCOL_PACKAGE_ARCHIVE_INVALID"),
            "协议包 ZIP 无法通过安全校验。",
            false,
        ),
        ProtocolPackageStorageErrorCode::CompilationFailed => (
            error
                .detail_code()
                .unwrap_or("PROTOCOL_PACKAGE_COMPILATION_FAILED"),
            "协议包声明或脚本无法通过编译。",
            false,
        ),
        ProtocolPackageStorageErrorCode::IdentityConflict => (
            "PROTOCOL_PACKAGE_IDENTITY_CONFLICT",
            "相同协议包 ID 与版本已经安装，但内容不同。",
            false,
        ),
        ProtocolPackageStorageErrorCode::NotFound => (
            "PROTOCOL_PACKAGE_NOT_FOUND",
            "指定的协议包精确版本尚未安装。",
            false,
        ),
        ProtocolPackageStorageErrorCode::StoredPackageInvalid => (
            error.detail_code().unwrap_or("PROTOCOL_PACKAGE_INVALID"),
            "已存储协议包无法通过完整重新校验。",
            false,
        ),
        ProtocolPackageStorageErrorCode::PersistenceFailed => (
            "PROTOCOL_PACKAGE_PERSISTENCE_FAILED",
            "协议包存储操作失败。",
            true,
        ),
    };
    let mut mapped = AppError::new(code, message);
    if retryable {
        mapped = mapped.retryable("请重试；若问题持续，请检查应用数据目录权限。");
    }
    if let Some(entity) = entity {
        mapped = mapped.entity(entity);
    }
    mapped
}
