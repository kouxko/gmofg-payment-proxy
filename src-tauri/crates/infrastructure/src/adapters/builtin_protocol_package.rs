//! 编译期内置 Wasm Component 到统一协议包注册表的适配器。

use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, BuiltinProtocolPackagePort, ExternalPackageApplicationPort,
    ProtocolPackageImportOutcomeViewModel, ProtocolPackageImportViewModel,
};
use intercept_proxy_package_runtime::read_package_component;

use super::{ExternalPackageRegistryAdapter, external_package_registry::application_description};

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
        let component = read_package_component(bytes).map_err(AppError::from)?;
        self.registry
            .install_local_archive(component.manifest(), bytes)
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
        let component = read_package_component(bytes).map_err(AppError::from)?;
        let outcome = self
            .registry
            .install_local_archive(component.manifest(), bytes)
            .await?;
        let package = component.manifest().package().identity();
        self.registry
            .activate_local_component(&package, bytes)
            .await?;
        let version = self.registry.get(&package).await?.ok_or_else(|| {
            AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "内置协议包写入后无法读取。")
        })?;
        let description = application_description(component.manifest());
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
