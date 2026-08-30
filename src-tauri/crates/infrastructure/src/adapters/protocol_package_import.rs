//! 原生 ZIP 文件选择到协议包注册表的导入适配器。
//!
//! Tauri/WebView 不提交路径或文件字节。平台对话框返回的本机路径在本适配器内受限读取，
//! 随后在本边界执行严格 ZIP/Manifest 校验。JavaScript/ESM 执行属于 Phase 8；在该运行时
//! 接入前，校验成功的包也会以稳定错误 fail closed，不再进入旧 TOML/Rhai 导入路径。

use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ProtocolPackageImportPort, ProtocolPackageImportPreviewViewModel,
    ProtocolPackageImportToken, ProtocolPackageImportViewModel,
};
use intercept_proxy_domain::{DomainError, ErrorCode};
use intercept_proxy_package_runtime::read_package_zip;

use crate::AtomicFileExporter;

use super::{NativeFileDialog, ProtocolPackageRepositoryAdapter, common::infra};

fn invalid_token() -> AppError {
    AppError::new(
        "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID",
        "协议包导入确认已过期、已使用或不是当前应用创建的令牌。",
    )
}

/// 把宿主原生文件选择器与协议包注册表组合成 Application 导入端口。
#[derive(Debug)]
pub struct ProtocolPackageImportAdapter {
    repository: Arc<ProtocolPackageRepositoryAdapter>,
    dialog: Arc<dyn NativeFileDialog>,
    files: AtomicFileExporter,
}

impl ProtocolPackageImportAdapter {
    #[must_use]
    pub fn new(
        repository: Arc<ProtocolPackageRepositoryAdapter>,
        dialog: Arc<dyn NativeFileDialog>,
    ) -> Self {
        Self {
            repository,
            dialog,
            files: AtomicFileExporter,
        }
    }
}

#[async_trait]
impl ProtocolPackageImportPort for ProtocolPackageImportAdapter {
    async fn prepare_zip(&self) -> AppResult<Option<ProtocolPackageImportPreviewViewModel>> {
        let Some(path) = self.dialog.choose_open_file("protocol_package_zip")? else {
            return Ok(None);
        };
        let bytes = infra(
            self.files
                .read_bounded(&path, self.repository.max_archive_bytes()),
        )?;
        let _archive = read_package_zip(
            std::io::Cursor::new(&bytes),
            self.repository.archive_limits(),
        )
        .map_err(AppError::from)?;
        return Err(AppError::from(
            DomainError::new(
                ErrorCode::ProtocolPackageInvalid,
                "JavaScript package execution is unavailable until the Sidecar runtime is installed",
            )
            .with_field_error("runtime", "Phase 8 JavaScript execution is not available"),
        ));
    }

    async fn commit_zip(
        &self,
        token: ProtocolPackageImportToken,
    ) -> AppResult<ProtocolPackageImportViewModel> {
        let _ = token;
        Err(invalid_token())
    }

    async fn discard_zip(&self, token: ProtocolPackageImportToken) -> AppResult<()> {
        let _ = token;
        Err(invalid_token())
    }
}

#[cfg(test)]
#[path = "protocol_package_import/tests.rs"]
mod tests;
