//! 原生 ZIP 文件选择到协议包注册表的导入适配器。
//!
//! Tauri/WebView 不提交路径或文件字节。平台对话框返回的本机路径在本适配器内受限读取，
//! 随后交给注册表执行 ZIP、Manifest、Schema、Rhai 和原子持久化整条校验链。

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ProtocolPackageImportDispositionViewModel, ProtocolPackageImportPort,
    ProtocolPackageImportPreviewViewModel, ProtocolPackageImportToken,
    ProtocolPackageImportViewModel,
};
use parking_lot::Mutex;
use uuid::Uuid;

use crate::AtomicFileExporter;

use super::{
    NativeFileDialog, PreparedProtocolPackage, ProtocolPackageInstallOutcome,
    ProtocolPackageRepositoryAdapter,
    common::infra,
    protocol_packages::{
        PreparedProtocolPackageDisposition, application_description, application_summary,
        protocol_package_app_error,
    },
};

const MAX_PENDING_IMPORTS: usize = 4;
const MAX_PENDING_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const PENDING_IMPORT_TTL: Duration = Duration::from_mins(5);

#[derive(Debug)]
struct PendingProtocolPackageImport {
    prepared: PreparedProtocolPackage,
    expires_at: Instant,
}

#[derive(Debug, Default)]
struct PendingProtocolPackageImports {
    entries: HashMap<ProtocolPackageImportToken, PendingProtocolPackageImport>,
    total_bytes: u64,
}

impl PendingProtocolPackageImports {
    fn remove_expired(&mut self, now: Instant) {
        self.entries.retain(|_, pending| pending.expires_at > now);
        self.total_bytes = self
            .entries
            .values()
            .map(|pending| pending.prepared.total_bytes())
            .sum();
    }

    fn insert(
        &mut self,
        prepared: PreparedProtocolPackage,
        now: Instant,
    ) -> AppResult<ProtocolPackageImportToken> {
        self.remove_expired(now);
        let next_total = self
            .total_bytes
            .checked_add(prepared.total_bytes())
            .ok_or_else(pending_limit_error)?;
        if self.entries.len() >= MAX_PENDING_IMPORTS || next_total > MAX_PENDING_TOTAL_BYTES {
            return Err(pending_limit_error());
        }
        let token = loop {
            let candidate = ProtocolPackageImportToken::from_uuid(Uuid::new_v4());
            if !self.entries.contains_key(&candidate) {
                break candidate;
            }
        };
        self.total_bytes = next_total;
        self.entries.insert(
            token,
            PendingProtocolPackageImport {
                prepared,
                expires_at: now + PENDING_IMPORT_TTL,
            },
        );
        Ok(token)
    }

    fn take(
        &mut self,
        token: ProtocolPackageImportToken,
        now: Instant,
    ) -> AppResult<PreparedProtocolPackage> {
        self.remove_expired(now);
        let pending = self.entries.remove(&token).ok_or_else(|| {
            AppError::new(
                "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID",
                "协议包导入确认已过期、已使用或不是当前应用创建的令牌。",
            )
        })?;
        self.total_bytes = self
            .total_bytes
            .saturating_sub(pending.prepared.total_bytes());
        Ok(pending.prepared)
    }

    fn discard(&mut self, token: ProtocolPackageImportToken, now: Instant) -> AppResult<()> {
        // discard 与 commit 使用完全相同的 take 语义：无论过期、重复还是伪造，都只返回同一
        // 稳定错误，既不泄漏 pending 集合状态，也不会让已释放 token 再次变为有效。
        self.take(token, now).map(drop)
    }
}

fn pending_limit_error() -> AppError {
    AppError::new(
        "PROTOCOL_PACKAGE_PENDING_LIMIT",
        "待确认的协议包导入过多或占用空间过大，请先完成已有导入。",
    )
}

/// 把宿主原生文件选择器与协议包注册表组合成 Application 导入端口。
#[derive(Debug)]
pub struct ProtocolPackageImportAdapter {
    repository: Arc<ProtocolPackageRepositoryAdapter>,
    dialog: Arc<dyn NativeFileDialog>,
    files: AtomicFileExporter,
    pending: Mutex<PendingProtocolPackageImports>,
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
            pending: Mutex::new(PendingProtocolPackageImports::default()),
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
        let prepared = self
            .repository
            .prepare_zip(&bytes)
            .map_err(|error| app_error_from_storage(&error))?;
        let compiled = prepared.compiled();
        let package = compiled.package().clone();
        let name = compiled.manifest().package().name().to_owned();
        let host_api = compiled.manifest().api();
        let description = application_description(compiled);
        let disposition = self
            .repository
            .prepared_disposition(&prepared)
            .map_err(|error| app_error_from_storage(&error))?;
        let (disposition, token) = match disposition {
            PreparedProtocolPackageDisposition::New => (
                ProtocolPackageImportDispositionViewModel::New,
                Some(self.pending.lock().insert(prepared, Instant::now())?),
            ),
            PreparedProtocolPackageDisposition::Reusable => (
                ProtocolPackageImportDispositionViewModel::Reusable,
                Some(self.pending.lock().insert(prepared, Instant::now())?),
            ),
            PreparedProtocolPackageDisposition::IdentityConflict => (
                ProtocolPackageImportDispositionViewModel::IdentityConflict,
                None,
            ),
        };
        Ok(Some(ProtocolPackageImportPreviewViewModel {
            token,
            disposition,
            package,
            name,
            host_api,
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
        let prepared = self.pending.lock().take(token, Instant::now())?;
        let description = application_description(prepared.compiled());
        let outcome = self
            .repository
            .install_prepared(prepared)
            .map_err(|error| app_error_from_storage(&error))?;
        let (outcome_kind, summary) = match outcome {
            ProtocolPackageInstallOutcome::Installed(summary) => (
                intercept_proxy_application::ProtocolPackageImportOutcomeViewModel::Installed,
                summary,
            ),
            ProtocolPackageInstallOutcome::Reused(summary) => (
                intercept_proxy_application::ProtocolPackageImportOutcomeViewModel::Reused,
                summary,
            ),
        };
        Ok(ProtocolPackageImportViewModel {
            outcome: outcome_kind,
            version: application_summary(summary),
            kind: description.kind,
            capabilities: description.capabilities,
            upstream_schema: description.upstream_schema,
            downstream_schema: description.downstream_schema,
        })
    }

    async fn discard_zip(&self, token: ProtocolPackageImportToken) -> AppResult<()> {
        self.pending.lock().discard(token, Instant::now())
    }
}

fn app_error_from_storage(
    error: &super::ProtocolPackageStorageError,
) -> intercept_proxy_application::AppError {
    // 存储错误需要保留 Archive/Rhai 的稳定细分码；文件系统错误才走公共 infra 映射。
    protocol_package_app_error(error)
}

#[cfg(test)]
#[path = "protocol_package_import/tests.rs"]
mod tests;
