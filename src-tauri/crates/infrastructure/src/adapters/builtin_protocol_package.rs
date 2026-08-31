//! 编译期内置严格 ZIP 到统一外部注册表的适配器。

use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, BuiltinProtocolPackagePort, ExternalPackageApplicationPort,
    ProtocolPackageImportOutcomeViewModel, ProtocolPackageImportViewModel,
};
use intercept_proxy_package_runtime::{PackageArchiveResourceLimits, read_package_zip};

use super::{ExternalPackageRegistryAdapter, external_package_registry::application_description};

#[derive(Debug, Clone, Copy)]
pub(crate) struct PackageArchiveLimits;

impl PackageArchiveResourceLimits for PackageArchiveLimits {
    fn max_archive_bytes(&self) -> u64 {
        8 * 1024 * 1024
    }
    fn max_entries(&self) -> usize {
        64
    }
    fn max_file_bytes(&self) -> u64 {
        1024 * 1024
    }
    fn max_total_bytes(&self) -> u64 {
        4 * 1024 * 1024
    }
    fn max_compression_ratio(&self) -> u64 {
        100
    }
    fn max_path_depth(&self) -> usize {
        8
    }
}

#[derive(Debug)]
pub struct BuiltinProtocolPackageAdapter {
    archive: Option<Arc<[u8]>>,
    registry: Arc<ExternalPackageRegistryAdapter>,
}

impl BuiltinProtocolPackageAdapter {
    #[must_use]
    pub fn new(archive: Option<Arc<[u8]>>, registry: Arc<ExternalPackageRegistryAdapter>) -> Self {
        Self { archive, registry }
    }

    pub async fn ensure_seeded(&self) -> AppResult<()> {
        let Some(bytes) = self.archive.as_deref() else {
            return Ok(());
        };
        let archive = read_package_zip(std::io::Cursor::new(bytes), &PackageArchiveLimits)
            .map_err(AppError::from)?;
        self.registry
            .install_local_archive(archive.manifest(), bytes)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl BuiltinProtocolPackagePort for BuiltinProtocolPackageAdapter {
    async fn builtin_archive(&self) -> AppResult<Vec<u8>> {
        self.archive
            .as_deref()
            .map(<[u8]>::to_vec)
            .ok_or_else(|| AppError::new("BUILTIN_PACKAGE_MISSING", "编译期内置协议包不存在。"))
    }

    async fn restore_builtin(&self) -> AppResult<ProtocolPackageImportViewModel> {
        let bytes = self
            .archive
            .as_deref()
            .ok_or_else(|| AppError::new("BUILTIN_PACKAGE_MISSING", "编译期内置协议包不存在。"))?;
        let archive = read_package_zip(std::io::Cursor::new(bytes), &PackageArchiveLimits)
            .map_err(AppError::from)?;
        let outcome = self
            .registry
            .install_local_archive(archive.manifest(), bytes)
            .await?;
        let package = archive.manifest().package().identity();
        let version = self.registry.get(&package).await?.ok_or_else(|| {
            AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "内置协议包写入后无法读取。")
        })?;
        let description = application_description(archive.manifest());
        let outcome = match outcome {
            crate::sqlite::external_packages::StoredLocalPackageInstallOutcome::Installed => ProtocolPackageImportOutcomeViewModel::Installed,
            crate::sqlite::external_packages::StoredLocalPackageInstallOutcome::Reused => ProtocolPackageImportOutcomeViewModel::Reused,
            crate::sqlite::external_packages::StoredLocalPackageInstallOutcome::IdentityConflict => return Err(AppError::new("PROTOCOL_PACKAGE_IDENTITY_CONFLICT", "相同协议包精确身份已存在不同内容。")),
        };
        Ok(ProtocolPackageImportViewModel {
            outcome,
            version,
            kind: description.kind,
            capabilities: description.capabilities,
            upstream_schema: description.upstream_schema,
            downstream_schema: description.downstream_schema,
        })
    }
}
