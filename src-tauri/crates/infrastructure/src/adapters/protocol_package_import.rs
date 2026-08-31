//! 原生 ZIP 文件选择到协议包注册表的导入适配器。
//!
//! Tauri/WebView 不提交路径或文件字节。平台对话框返回的本机路径在本适配器内受限读取，
//! 随后在本边界执行严格 ZIP/Manifest/resources 校验；提交后由本地 Sidecar
//! 进程主动连接统一 `/packages` WebSocket 注册，不进入旧 JSON/JavaScript 导入路径。

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ExternalPackageApplicationPort, ProtocolPackageImportDispositionViewModel,
    ProtocolPackageImportOutcomeViewModel, ProtocolPackageImportPort,
    ProtocolPackageImportPreviewViewModel, ProtocolPackageImportToken,
    ProtocolPackageImportViewModel,
};
use intercept_proxy_package_contract::PackageManifest;
use intercept_proxy_package_runtime::read_package_zip;
use parking_lot::{Mutex, RwLock};
use uuid::Uuid;

use crate::AtomicFileExporter;

use super::external_package_registry::application_description;
use super::{
    ExternalPackageRegistryAdapter, LocalPackageSupervisor, NativeFileDialog, PackageArchiveLimits,
    common::infra,
};
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
    supervisor: RwLock<Option<Arc<LocalPackageSupervisor>>>,
}

#[derive(Debug)]
struct PendingLocalPackage {
    manifest: PackageManifest,
    archive: Vec<u8>,
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
            supervisor: RwLock::new(None),
        }
    }

    pub(crate) fn set_supervisor(&self, supervisor: Arc<LocalPackageSupervisor>) {
        *self.supervisor.write() = Some(supervisor);
    }
}

#[async_trait]
impl ProtocolPackageImportPort for ProtocolPackageImportAdapter {
    async fn prepare_zip(&self) -> AppResult<Option<ProtocolPackageImportPreviewViewModel>> {
        let Some(path) = self.dialog.choose_open_file("protocol_package_zip")? else {
            return Ok(None);
        };
        let bytes = infra(self.files.read_bounded(&path, 8 * 1024 * 1024))?;
        let archive = read_package_zip(std::io::Cursor::new(&bytes), &PackageArchiveLimits)
            .map_err(AppError::from)?;
        let manifest = archive.manifest().clone();
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
                    archive: bytes,
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

    async fn commit_zip(
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
            .install_local_archive(&pending.manifest, &pending.archive)
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
        let supervisor = self.supervisor.read().clone().ok_or_else(|| {
            AppError::new(
                "EXTERNAL_PACKAGE_PROCESS_FAILED",
                "本地软件包进程监督器尚未启动。",
            )
        })?;
        if let Err(error) = supervisor.launch(package.clone(), &pending.archive).await {
            registry.record_package_operation_failure(
                "local_sidecar_import_start",
                &package,
                &error,
            );
        }
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

    async fn discard_zip(&self, token: ProtocolPackageImportToken) -> AppResult<()> {
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
