//! 编译期内置 Wasm Component 到统一协议包注册表的适配器。

use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::{AppError, AppResult, BuiltinProtocolPackagePort};
use intercept_proxy_package_runtime::read_package_component;

use super::ExternalPackageRegistryAdapter;

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
}
