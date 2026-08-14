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
    AppError, AppResult, ProtocolPackageImportPort, ProtocolPackageImportPreviewViewModel,
    ProtocolPackageImportToken, ProtocolPackageImportViewModel,
};
use parking_lot::Mutex;
use uuid::Uuid;

use crate::AtomicFileExporter;

use super::{
    NativeFileDialog, PreparedProtocolPackage, ProtocolPackageInstallOutcome,
    ProtocolPackageRepositoryAdapter,
    common::infra,
    protocol_packages::{application_description, application_summary, protocol_package_app_error},
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
        let token = self.pending.lock().insert(prepared, Instant::now())?;
        Ok(Some(ProtocolPackageImportPreviewViewModel {
            token,
            package,
            name,
            host_api,
            capabilities: description.capabilities,
            schema: description.schema,
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
            capabilities: description.capabilities,
            schema: description.schema,
        })
    }
}

fn app_error_from_storage(
    error: &super::ProtocolPackageStorageError,
) -> intercept_proxy_application::AppError {
    // 存储错误需要保留 Archive/Rhai 的稳定细分码；文件系统错误才走公共 infra 映射。
    protocol_package_app_error(error)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Cursor, Write},
        path::PathBuf,
        sync::Mutex,
    };

    use intercept_proxy_application::{
        AppError, AppResult, ProtocolPackageImportOutcomeViewModel, ProtocolPackageImportPort,
    };
    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::{SqliteStore, adapters::FileSelection};

    const MANIFEST: &str = r#"
api = 1

[package]
id = "example-protocol"
name = "Example Protocol"
version = "1.0.0"

[document]
schema = "document.toml"
display = { script = "protocol.rhai", function = "display" }

[hooks.upstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"

[hooks.upstream.send]
script = "protocol.rhai"
encode = "encode"

[hooks.downstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"
"#;

    const SCHEMA: &str = r#"
id = "example-message"
version = 1
title = "Example Message"

[[fields]]
name = "trace_id"
label = "Trace ID"
type = "string"

[[fields]]
name = "amount"
label = "Amount"
type = "int"

[[fields]]
name = "approved"
label = "Approved"
type = "bool"
"#;

    const SCRIPT: &str = r#"
fn frame(reader, context) { framing::complete(1) }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
fn display(document, context) { "<p>ok</p>" }
"#;

    #[derive(Debug, Default)]
    struct QueueDialog {
        paths: Mutex<Vec<AppResult<Option<PathBuf>>>>,
        purposes: Mutex<Vec<String>>,
    }

    impl QueueDialog {
        fn push(&self, path: AppResult<Option<PathBuf>>) {
            self.paths.lock().unwrap().push(path);
        }
    }

    impl NativeFileDialog for QueueDialog {
        fn choose_open_file(&self, purpose: &str) -> AppResult<Option<PathBuf>> {
            self.purposes.lock().unwrap().push(purpose.to_owned());
            self.paths.lock().unwrap().remove(0)
        }

        fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
            unreachable!("protocol package import never opens a save dialog")
        }
    }

    #[tokio::test]
    async fn native_dialog_cancellation_and_permission_errors_never_change_the_registry() {
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
            Arc::clone(&store),
        ));
        let dialog = Arc::new(QueueDialog::default());
        dialog.push(Ok(None));
        dialog.push(Err(AppError::new(
            "FILE_DIALOG_PERMISSION_DENIED",
            "denied",
        )));
        let importer = ProtocolPackageImportAdapter::new(repository.clone(), dialog.clone());

        assert_eq!(importer.prepare_zip().await.unwrap(), None);
        let error = importer.prepare_zip().await.unwrap_err();
        assert_eq!(error.view_model.code, "FILE_DIALOG_PERMISSION_DENIED");
        assert!(repository.list().unwrap().is_empty());
        assert_eq!(store.protocol_package_row_counts_for_test(), (0, 0));
        assert_eq!(
            dialog.purposes.lock().unwrap().as_slice(),
            ["protocol_package_zip", "protocol_package_zip"]
        );
    }

    #[tokio::test]
    async fn invalid_zip_and_rhai_are_rejected_without_partial_state() {
        let fixture = TempDir::new().unwrap();
        let invalid_zip = fixture.path().join("invalid.zip");
        std::fs::write(&invalid_zip, b"not a zip").unwrap();
        let invalid_rhai = fixture.path().join("invalid-rhai.zip");
        std::fs::write(&invalid_rhai, package_zip("fn frame( {")).unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
            Arc::clone(&store),
        ));
        let dialog = Arc::new(QueueDialog::default());
        dialog.push(Ok(Some(invalid_zip)));
        dialog.push(Ok(Some(invalid_rhai)));
        let importer = ProtocolPackageImportAdapter::new(repository.clone(), dialog);

        assert_eq!(
            importer.prepare_zip().await.unwrap_err().view_model.code,
            "INVALID_ZIP"
        );
        assert_eq!(
            importer.prepare_zip().await.unwrap_err().view_model.code,
            "SCRIPT_SYNTAX_INVALID"
        );
        assert!(repository.list().unwrap().is_empty());
        assert_eq!(store.protocol_package_row_counts_for_test(), (0, 0));
    }

    #[tokio::test]
    async fn valid_zip_returns_safe_capabilities_schema_and_idempotent_outcome() {
        let fixture = TempDir::new().unwrap();
        let path = fixture.path().join("example.zip");
        std::fs::write(&path, package_zip(SCRIPT)).unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(store));
        let dialog = Arc::new(QueueDialog::default());
        dialog.push(Ok(Some(path.clone())));
        dialog.push(Ok(Some(path)));
        let importer = ProtocolPackageImportAdapter::new(repository, dialog);

        let first_preview = importer.prepare_zip().await.unwrap().unwrap();
        assert!(
            importer.repository.list().unwrap().is_empty(),
            "prepare must not install before explicit confirmation"
        );
        let installed = importer.commit_zip(first_preview.token).await.unwrap();
        let second_preview = importer.prepare_zip().await.unwrap().unwrap();
        let reused = importer.commit_zip(second_preview.token).await.unwrap();
        assert_eq!(
            installed.outcome,
            ProtocolPackageImportOutcomeViewModel::Installed
        );
        assert_eq!(
            reused.outcome,
            ProtocolPackageImportOutcomeViewModel::Reused
        );
        assert_eq!(installed.version.package, reused.version.package);
        assert!(installed.capabilities.upstream.encode);
        assert!(!installed.capabilities.downstream.encode);
        assert!(installed.capabilities.display);
        assert_eq!(
            installed
                .schema
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["trace_id", "amount", "approved"]
        );
    }

    #[tokio::test]
    async fn commit_uses_frozen_validated_files_and_token_is_single_use() {
        let fixture = TempDir::new().unwrap();
        let path = fixture.path().join("replace-after-prepare.zip");
        std::fs::write(&path, package_zip(SCRIPT)).unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(store));
        let dialog = Arc::new(QueueDialog::default());
        dialog.push(Ok(Some(path.clone())));
        let importer = ProtocolPackageImportAdapter::new(repository.clone(), dialog);

        let preview = importer.prepare_zip().await.unwrap().unwrap();
        // prepare 后即使磁盘原 ZIP 被替换，commit 也只能提交内存中被冻结且已验证的规范文件。
        std::fs::write(&path, b"not the validated archive").unwrap();
        let installed = importer.commit_zip(preview.token).await.unwrap();
        assert_eq!(installed.version.package, preview.package);
        assert!(repository.summary(&preview.package).unwrap().is_some());

        let reused_token = importer.commit_zip(preview.token).await.unwrap_err();
        assert_eq!(
            reused_token.view_model.code,
            "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
        );
        let forged = ProtocolPackageImportToken::from_uuid(Uuid::new_v4());
        assert_eq!(
            importer
                .commit_zip(forged)
                .await
                .unwrap_err()
                .view_model
                .code,
            "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
        );
    }

    #[test]
    fn pending_imports_expire_and_enforce_the_count_limit() {
        let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
            SqliteStore::in_memory().unwrap(),
        ));
        let now = Instant::now();
        let mut pending = PendingProtocolPackageImports::default();
        let first = pending
            .insert(repository.prepare_zip(&package_zip(SCRIPT)).unwrap(), now)
            .unwrap();
        assert_eq!(
            pending
                .take(first, now + PENDING_IMPORT_TTL)
                .unwrap_err()
                .view_model
                .code,
            "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
        );

        for _ in 0..MAX_PENDING_IMPORTS {
            pending
                .insert(repository.prepare_zip(&package_zip(SCRIPT)).unwrap(), now)
                .unwrap();
        }
        let error = pending
            .insert(repository.prepare_zip(&package_zip(SCRIPT)).unwrap(), now)
            .unwrap_err();
        assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_PENDING_LIMIT");
    }

    fn package_zip(script: &str) -> Vec<u8> {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        for (path, contents) in [
            ("manifest.toml", MANIFEST.as_bytes()),
            ("document.toml", SCHEMA.as_bytes()),
            ("protocol.rhai", script.as_bytes()),
        ] {
            archive
                .start_file(path, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }
}
