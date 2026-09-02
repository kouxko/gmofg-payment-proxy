//! Deterministic ZIP construction and durable atomic application backup write.

use std::{
    fmt,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ApplicationBackupExportOutcome, ApplicationBackupExportPort,
    ApplicationBackupExportSnapshot, serialize_application_backup_document,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    ApplicationBackupArchive, ApplicationBackupFileSystem, AtomicFileExporter, InfrastructureError,
    SystemApplicationBackupFileSystem,
};

pub struct ApplicationBackupFileExporter {
    target: PathBuf,
    overwrite: bool,
    file_system: Arc<dyn ApplicationBackupFileSystem>,
}

impl ApplicationBackupFileExporter {
    #[must_use]
    pub fn new(target: PathBuf, overwrite: bool) -> Self {
        Self::with_file_system(
            target,
            overwrite,
            Arc::new(SystemApplicationBackupFileSystem),
        )
    }

    #[doc(hidden)]
    #[must_use]
    pub fn with_file_system(
        target: PathBuf,
        overwrite: bool,
        file_system: Arc<dyn ApplicationBackupFileSystem>,
    ) -> Self {
        Self {
            target,
            overwrite,
            file_system,
        }
    }
}

impl fmt::Debug for ApplicationBackupFileExporter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationBackupFileExporter")
            .field("target_selected", &true)
            .field("overwrite", &self.overwrite)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl ApplicationBackupExportPort for ApplicationBackupFileExporter {
    async fn write(
        &self,
        snapshot: ApplicationBackupExportSnapshot,
    ) -> AppResult<ApplicationBackupExportOutcome> {
        let target = self.target.clone();
        let overwrite = self.overwrite;
        let file_system = Arc::clone(&self.file_system);
        tokio::task::spawn_blocking(move || {
            write_application_backup(&target, overwrite, file_system.as_ref(), &snapshot)
        })
        .await
        .map_err(|_| {
            AppError::new(
                "APPLICATION_BACKUP_EXPORT_FAILED",
                "应用备份后台写入任务未能完成。",
            )
        })?
    }
}

fn write_application_backup(
    target: &Path,
    overwrite: bool,
    file_system: &dyn ApplicationBackupFileSystem,
    snapshot: &ApplicationBackupExportSnapshot,
) -> AppResult<ApplicationBackupExportOutcome> {
    let bytes = build_application_backup_zip(snapshot)?;
    let outcome = AtomicFileExporter
        .write_with_file_system(target, &bytes, overwrite, file_system)
        .map_err(|error| export_error(&error))?;
    Ok(ApplicationBackupExportOutcome {
        bytes_written: outcome.bytes_written,
        replaced_existing: outcome.replaced_existing,
    })
}

pub fn build_application_backup_zip(
    snapshot: &ApplicationBackupExportSnapshot,
) -> AppResult<Vec<u8>> {
    snapshot.document.validate()?;
    if snapshot.document.referenced_paths() != snapshot.files.keys().cloned().collect() {
        return Err(AppError::new(
            "APPLICATION_BACKUP_EXPORT_INVALID",
            "应用备份文件引用与导出快照不一致。",
        ));
    }

    let application = serialize_application_backup_document(&snapshot.document)?;
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    write_entry(&mut writer, "application.json", &application, options)?;
    for (path, bytes) in &snapshot.files {
        write_entry(&mut writer, path.as_str(), bytes, options)?;
    }
    let bytes = writer.finish().map_err(|_| zip_build_error())?.into_inner();
    ApplicationBackupArchive::read(&bytes).map_err(|_| zip_build_error())?;
    Ok(bytes)
}

fn write_entry<W: Write + std::io::Seek>(
    writer: &mut ZipWriter<W>,
    path: &str,
    bytes: &[u8],
    options: SimpleFileOptions,
) -> AppResult<()> {
    writer
        .start_file(path, options)
        .map_err(|_| zip_build_error())?;
    writer.write_all(bytes).map_err(|_| zip_build_error())
}

fn zip_build_error() -> AppError {
    AppError::new(
        "APPLICATION_BACKUP_EXPORT_FAILED",
        "应用备份 ZIP 构建失败。",
    )
}

fn export_error(error: &InfrastructureError) -> AppError {
    match error {
        InfrastructureError::ExportTargetExists { .. } => AppError::new(
            "APPLICATION_BACKUP_EXPORT_TARGET_EXISTS",
            "目标文件已存在，未执行覆盖。",
        ),
        InfrastructureError::ExportParentSync { .. } => AppError::new(
            "APPLICATION_BACKUP_EXPORT_DURABILITY_UNCERTAIN",
            "目标文件已替换，但崩溃恢复所需的目录持久化状态无法确认。",
        ),
        _ => AppError::new(
            "APPLICATION_BACKUP_EXPORT_FAILED",
            "应用备份文件写入失败，原目标未被修改。",
        ),
    }
}
