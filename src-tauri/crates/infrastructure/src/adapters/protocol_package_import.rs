//! 原生 WebAssembly Component 文件选择到协议包注册表的导入适配器。
//!
//! Tauri/WebView 不提交路径或文件字节。平台对话框返回的本机路径在本适配器内受限读取，
//! 随后在本边界执行严格 Component/Manifest/world 校验；提交后由主进程内 Wasmtime
//! runtime 直接提供协议 Hook，不进入远端 `/packages` WebSocket 路径。

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ExternalPackageApplicationPort, ProtocolPackageImportDispositionViewModel,
    ProtocolPackageImportOutcomeViewModel, ProtocolPackageImportPort,
    ProtocolPackageImportPreviewViewModel, ProtocolPackageImportToken,
    ProtocolPackageImportViewModel,
};
use intercept_proxy_package_contract::PackageManifest;
use intercept_proxy_package_runtime::read_package_component;
use parking_lot::Mutex;
use uuid::Uuid;

use crate::AtomicFileExporter;

use super::external_package_registry::application_description;
use super::{ExternalPackageRegistryAdapter, NativeFileDialog, common::infra};
use crate::sqlite::external_packages::StoredLocalPackageInstallOutcome;

fn invalid_token() -> AppError {
    AppError::new(
        "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID",
        "协议包导入确认已过期、已使用或不是当前应用创建的令牌。",
    )
}

/// 把宿主原生文件选择器与协议包注册表组合成 Application 导入端口。
#[derive(Debug)]
pub struct ProtocolPackageImportAdapter {
    registry: Arc<ExternalPackageRegistryAdapter>,
    dialog: Arc<dyn NativeFileDialog>,
    files: AtomicFileExporter,
    pending: Mutex<HashMap<Uuid, PendingLocalPackage>>,
}

#[derive(Debug)]
struct PendingLocalPackage {
    manifest: PackageManifest,
    component: Vec<u8>,
}

impl ProtocolPackageImportAdapter {
    #[must_use]
    pub fn new(
        registry: Arc<ExternalPackageRegistryAdapter>,
        dialog: Arc<dyn NativeFileDialog>,
    ) -> Self {
        Self {
            registry,
            dialog,
            files: AtomicFileExporter,
            pending: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ProtocolPackageImportPort for ProtocolPackageImportAdapter {
    async fn prepare_component(&self) -> AppResult<Option<ProtocolPackageImportPreviewViewModel>> {
        let Some(path) = self.dialog.choose_open_file("protocol_package_wasm")? else {
            return Ok(None);
        };
        let bytes = infra(self.files.read_bounded(&path, u64::MAX))?;
        let component = read_package_component(&bytes).map_err(AppError::from)?;
        let manifest = component.manifest().clone();
        let disposition = match self
            .registry
            .preview_local_archive(&manifest, &bytes)
            .await?
        {
            StoredLocalPackageInstallOutcome::Installed => {
                ProtocolPackageImportDispositionViewModel::New
            }
            StoredLocalPackageInstallOutcome::Reused => {
                ProtocolPackageImportDispositionViewModel::Reusable
            }
            StoredLocalPackageInstallOutcome::IdentityConflict => {
                ProtocolPackageImportDispositionViewModel::IdentityConflict
            }
        };
        let token = if disposition == ProtocolPackageImportDispositionViewModel::IdentityConflict {
            None
        } else {
            let token = ProtocolPackageImportToken::from_uuid(Uuid::new_v4());
            self.pending.lock().insert(
                token.as_uuid(),
                PendingLocalPackage {
                    manifest: manifest.clone(),
                    component: bytes,
                },
            );
            Some(token)
        };
        let description = application_description(&manifest);
        Ok(Some(ProtocolPackageImportPreviewViewModel {
            token,
            disposition,
            package: manifest.package().identity(),
            name: manifest.package().name().to_owned(),
            host_api: manifest.api(),
            kind: description.kind,
            capabilities: description.capabilities,
            upstream_schema: description.upstream_schema,
            downstream_schema: description.downstream_schema,
        }))
    }

    async fn commit_component(
        &self,
        token: ProtocolPackageImportToken,
    ) -> AppResult<ProtocolPackageImportViewModel> {
        let pending = self
            .pending
            .lock()
            .remove(&token.as_uuid())
            .ok_or_else(invalid_token)?;
        let registry = Arc::clone(&self.registry);
        let outcome = registry
            .install_and_activate_local_component(&pending.manifest, &pending.component)
            .await?;
        let outcome = match outcome {
            StoredLocalPackageInstallOutcome::Installed => {
                ProtocolPackageImportOutcomeViewModel::Installed
            }
            StoredLocalPackageInstallOutcome::Reused => {
                ProtocolPackageImportOutcomeViewModel::Reused
            }
            StoredLocalPackageInstallOutcome::IdentityConflict => {
                return Err(AppError::new(
                    "PROTOCOL_PACKAGE_IDENTITY_CONFLICT",
                    "相同协议包精确身份已存在不同内容。",
                ));
            }
        };
        let package = pending.manifest.package().identity();
        let version = registry.get(&package).await?.ok_or_else(|| {
            AppError::new(
                "PROTOCOL_PACKAGE_NOT_FOUND",
                "本地软件包提交后未找到精确版本。",
            )
        })?;
        let description = application_description(&pending.manifest);
        Ok(ProtocolPackageImportViewModel {
            outcome,
            version,
            kind: description.kind,
            capabilities: description.capabilities,
            upstream_schema: description.upstream_schema,
            downstream_schema: description.downstream_schema,
        })
    }

    async fn discard_component(&self, token: ProtocolPackageImportToken) -> AppResult<()> {
        self.pending
            .lock()
            .remove(&token.as_uuid())
            .map(|_| ())
            .ok_or_else(invalid_token)
    }
}

#[cfg(test)]
#[path = "protocol_package_import/tests.rs"]
mod tests;
