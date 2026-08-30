//! 基础设施协议包注册表到 Application 生命周期端口的映射。
//!
//! 此处只转换无源码元数据与稳定错误；启停、引用和删除规则仍由 Application 用例决定。

use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppErrorDiagnosticViewModel, AppResult, ProtocolPackageCapabilitiesViewModel,
    ProtocolPackageCompilationReceipt, ProtocolPackageCompilerPort,
    ProtocolPackageDescriptionViewModel, ProtocolPackageDirectionCapabilitiesViewModel,
    ProtocolPackageKindViewModel, ProtocolPackageSchemaViewModel, ProtocolPackageSourceViewModel,
    ProtocolPackageStorePort, ProtocolPackageValidationViewModel, ProtocolPackageVersionViewModel,
    is_builtin_protocol_package,
};
use intercept_proxy_protocol_scripting::CompiledProtocolPackage;
use intercept_proxy_protocol_scripting::{ProtocolDirection, ProtocolPackageKind};

use super::{
    PreparedProtocolPackage, ProtocolPackageRef, ProtocolPackageRepositoryAdapter,
    ProtocolPackageStorageError, ProtocolPackageStorageErrorCode, ProtocolPackageSummary,
    ProtocolPackageValidationStatus,
};

#[async_trait]
impl ProtocolPackageStorePort for ProtocolPackageRepositoryAdapter {
    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        self.executor
            .execute(|store| {
                store
                    .list_protocol_package_headers()
                    .map(|headers| {
                        headers
                            .into_iter()
                            .map(super::summary_from_header)
                            .collect::<Vec<ProtocolPackageSummary>>()
                    })
                    .map_err(ProtocolPackageStorageError::from)
            })
            .await
            .map(|items| items.into_iter().map(application_summary).collect())
            .map_err(|error| protocol_package_app_error(&error))
    }

    async fn get(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        let selected = package.clone();
        self.executor
            .execute(move |store| {
                store
                    .load_protocol_package_header(&selected)
                    .map(|header| header.map(super::summary_from_header))
                    .map_err(ProtocolPackageStorageError::from)
            })
            .await
            .map(|summary| summary.map(application_summary))
            .map_err(|error| protocol_package_app_error(&error))
    }

    async fn set_enabled(&self, package: &ProtocolPackageRef, enabled: bool) -> AppResult<()> {
        let selected = package.clone();
        self.executor
            .execute(move |store| {
                if store.set_protocol_package_enabled(&selected, enabled)? {
                    Ok(())
                } else {
                    Err(ProtocolPackageStorageError::NotFound { package: selected })
                }
            })
            .await
            .map_err(|error| protocol_package_app_error(&error))
    }

    async fn delete(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        let selected = package.clone();
        let cache = Arc::clone(&self.cache);
        self.executor
            .execute(move |store| {
                let mut cache = cache.lock();
                if store.delete_protocol_package(&selected)? {
                    cache.remove(&selected);
                    Ok(())
                } else {
                    Err(ProtocolPackageStorageError::NotFound { package: selected })
                }
            })
            .await
            .map_err(|error| protocol_package_app_error(&error))
    }
}

#[async_trait]
impl ProtocolPackageCompilerPort for ProtocolPackageRepositoryAdapter {
    async fn compile_fresh(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageCompilationReceipt> {
        let compiled = self
            .revalidate_async(package)
            .await
            .map_err(|error| protocol_package_app_error(&error))?;
        Ok(ProtocolPackageCompilationReceipt {
            package: compiled.package().clone(),
            host_api: compiled.manifest().api(),
            // `revalidate()` 走与导入相同的 Manifest parser；不受当前 Host 支持的 API 会在
            // 到达这里之前以稳定编译错误失败。
            compatible: true,
        })
    }

    async fn describe(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        self.revalidate_async(package)
            .await
            .map(|compiled| application_description(&compiled))
            .map_err(|error| protocol_package_app_error(&error))
    }
}

pub(in crate::adapters) fn application_summary(
    summary: ProtocolPackageSummary,
) -> ProtocolPackageVersionViewModel {
    let built_in = is_builtin_protocol_package(&summary.package);
    ProtocolPackageVersionViewModel {
        package: summary.package,
        name: summary.name,
        host_api: summary.host_api,
        kind: match summary.kind {
            ProtocolPackageKind::Http => ProtocolPackageKindViewModel::Http,
            ProtocolPackageKind::Socket => ProtocolPackageKindViewModel::Socket,
        },
        source: ProtocolPackageSourceViewModel::Internal { built_in },
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

pub(in crate::adapters) fn application_description(
    compiled: &CompiledProtocolPackage,
) -> ProtocolPackageDescriptionViewModel {
    let frame = compiled.kind() == ProtocolPackageKind::Socket;
    ProtocolPackageDescriptionViewModel {
        package: compiled.package().clone(),
        kind: match compiled.kind() {
            ProtocolPackageKind::Http => ProtocolPackageKindViewModel::Http,
            ProtocolPackageKind::Socket => ProtocolPackageKindViewModel::Socket,
        },
        capabilities: ProtocolPackageCapabilitiesViewModel {
            upstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame,
                decode: true,
                encode: compiled.supports_upstream_encode(),
            },
            downstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame,
                decode: true,
                encode: compiled.supports_downstream_encode(),
            },
            display: compiled.supports_display(),
        },
        upstream_schema: Some(application_schema(
            compiled.schema(ProtocolDirection::Upstream),
        )),
        downstream_schema: Some(application_schema(
            compiled.schema(ProtocolDirection::Downstream),
        )),
    }
}

fn application_schema(
    schema: &intercept_proxy_domain::DocumentSchemaNode,
) -> ProtocolPackageSchemaViewModel {
    ProtocolPackageSchemaViewModel {
        root: schema.clone(),
    }
}

pub(in crate::adapters) fn application_descriptions(
    packages: &[PreparedProtocolPackage],
) -> Vec<ProtocolPackageDescriptionViewModel> {
    packages
        .iter()
        .map(|package| application_description(package.compiled()))
        .collect()
}

pub(in crate::adapters) fn protocol_package_app_error(
    error: &ProtocolPackageStorageError,
) -> AppError {
    let entity = match error {
        ProtocolPackageStorageError::IdentityConflict { package }
        | ProtocolPackageStorageError::NotFound { package }
        | ProtocolPackageStorageError::StoredPackageInvalid { package, .. } => {
            Some(format!("{}@{}", package.id, package.version))
        }
        ProtocolPackageStorageError::Archive { .. }
        | ProtocolPackageStorageError::PortableInvalid
        | ProtocolPackageStorageError::Compilation { .. }
        | ProtocolPackageStorageError::Infrastructure(_) => None,
    };
    let (code, message, retryable) = match error.code() {
        ProtocolPackageStorageErrorCode::ArchiveInvalid => {
            let message = if matches!(error, ProtocolPackageStorageError::PortableInvalid) {
                "可移植协议包文件载荷无法通过安全校验。"
            } else {
                "协议包 ZIP 无法通过安全校验。"
            };
            (
                error
                    .detail_code()
                    .unwrap_or("PROTOCOL_PACKAGE_ARCHIVE_INVALID"),
                message,
                false,
            )
        }
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
    if let Some(diagnostic) = import_diagnostic(error) {
        mapped = mapped.diagnostic(diagnostic);
    }
    mapped
}

fn import_diagnostic(error: &ProtocolPackageStorageError) -> Option<AppErrorDiagnosticViewModel> {
    match error {
        ProtocolPackageStorageError::Archive { source } => Some(AppErrorDiagnosticViewModel {
            file: source.path().map(|path| path.as_str().to_owned()),
            field: None,
            line: None,
            column: None,
            entry: None,
        }),
        ProtocolPackageStorageError::Compilation { source } => {
            if let Some(declaration) = source.declaration_error() {
                return Some(AppErrorDiagnosticViewModel {
                    file: Some(declaration.file().file_name().to_owned()),
                    field: Some(declaration.field().to_owned()),
                    line: None,
                    column: None,
                    entry: None,
                });
            }
            source
                .script_error()
                .map(|script| AppErrorDiagnosticViewModel {
                    file: script.file().map(|path| path.as_str().to_owned()),
                    field: None,
                    line: script.line().and_then(|line| u32::try_from(line).ok()),
                    column: script
                        .column()
                        .and_then(|column| u32::try_from(column).ok()),
                    entry: script.entry().map(|entry| entry.as_str().to_owned()),
                })
        }
        ProtocolPackageStorageError::PortableInvalid
        | ProtocolPackageStorageError::IdentityConflict { .. }
        | ProtocolPackageStorageError::NotFound { .. }
        | ProtocolPackageStorageError::StoredPackageInvalid { .. }
        | ProtocolPackageStorageError::Infrastructure(_) => None,
    }
}
