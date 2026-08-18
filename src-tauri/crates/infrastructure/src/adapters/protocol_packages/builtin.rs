//! 官方 ISO 8583 起始示例的内置 ZIP 生命周期。

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, BuiltinProtocolPackagePort, ProtocolPackageImportOutcomeViewModel,
    ProtocolPackageImportViewModel, builtin_iso8583_package_ref,
};
use serde_json::Value;
use uuid::Uuid;

use super::{
    CachedCompiledPackage, PreparedProtocolPackage, ProtocolPackageRepositoryAdapter,
    ProtocolPackageStorageError, application_description, application_summary,
    protocol_package_app_error,
};
use crate::WorkspaceRecord;
use crate::sqlite::protocol_packages::{
    StoredBuiltinSeedOutcome, StoredProtocolPackageHeader, StoredProtocolPackageValidation,
    StoredProtocolPackageWrite,
};

impl ProtocolPackageRepositoryAdapter {
    #[must_use]
    pub fn with_builtin_archive(mut self, archive: Arc<[u8]>) -> Self {
        self.builtin_archive = Some(archive);
        self
    }

    /// 原生文件读取与内置资产共用同一 Archive 压缩字节上限。
    #[must_use]
    pub const fn max_archive_bytes(&self) -> u64 {
        self.archive_limits.max_archive_bytes()
    }

    /// 只在独立 feature marker 尚未提交时安装官方包。
    pub fn ensure_builtin_seeded(&self) -> AppResult<()> {
        if self.builtin_archive.is_none() {
            return Ok(());
        }
        let prepared = self.prepare_builtin()?;
        let header = builtin_header(&prepared);
        let package = header.package.clone();
        let mut cache = self.cache.lock();
        let outcome = self
            .store
            .seed_builtin_protocol_package(&header, &prepared.files)
            .map_err(super::portability::bundle_app_error)?;
        if let StoredBuiltinSeedOutcome::Ready(generation) = outcome {
            cache.insert(
                package,
                CachedCompiledPackage {
                    generation,
                    compiled: Arc::new(prepared.compiled),
                },
            );
        }
        Ok(())
    }

    pub(super) fn prepare_builtin(&self) -> AppResult<PreparedProtocolPackage> {
        let archive = self.builtin_archive.as_deref().ok_or_else(|| {
            AppError::new(
                "BUILTIN_PROTOCOL_PACKAGE_UNAVAILABLE",
                "当前宿主未提供官方 ISO 8583 起始示例。",
            )
        })?;
        let prepared = self
            .prepare_zip(archive)
            .map_err(|error| protocol_package_app_error(&error))?;
        if prepared.compiled.package() != &builtin_iso8583_package_ref() {
            return Err(AppError::new(
                "BUILTIN_PROTOCOL_PACKAGE_IDENTITY_INVALID",
                "内置协议包与应用保护的精确身份不一致。",
            ));
        }
        Ok(prepared)
    }

    pub(super) fn reset_with_builtin(
        &self,
        selected_workspace_id: Uuid,
        records: &[WorkspaceRecord],
        settings: &Value,
    ) -> AppResult<()> {
        let builtin = if self.builtin_archive.is_some() {
            Some(self.prepare_builtin()?)
        } else {
            None
        };
        let builtin_write = builtin.as_ref().map(|prepared| StoredProtocolPackageWrite {
            header: builtin_header(prepared),
            files: prepared.files.clone(),
        });
        let mut cache = self.cache.lock();
        self.store
            .reset_application_bundle(
                selected_workspace_id,
                records,
                settings,
                builtin_write.as_ref(),
            )
            .map_err(ProtocolPackageStorageError::from)
            .map_err(|error| protocol_package_app_error(&error))?;
        cache.clear();
        if let (Some(prepared), Some(write)) = (builtin, builtin_write) {
            cache.insert(
                write.header.package,
                CachedCompiledPackage {
                    generation: write.header.generation,
                    compiled: Arc::new(prepared.compiled),
                },
            );
        }
        Ok(())
    }
}

#[async_trait]
impl BuiltinProtocolPackagePort for ProtocolPackageRepositoryAdapter {
    async fn builtin_archive(&self) -> AppResult<Vec<u8>> {
        self.builtin_archive
            .as_deref()
            .map(<[u8]>::to_vec)
            .ok_or_else(|| {
                AppError::new(
                    "BUILTIN_PROTOCOL_PACKAGE_UNAVAILABLE",
                    "当前宿主未提供官方 ISO 8583 起始示例。",
                )
            })
    }

    async fn restore_builtin(&self) -> AppResult<ProtocolPackageImportViewModel> {
        let prepared = self.prepare_builtin()?;
        let description = application_description(&prepared.compiled);
        let header = builtin_header(&prepared);
        let package = header.package.clone();
        let mut cache = self.cache.lock();
        let generation = self
            .store
            .restore_builtin_protocol_package(&header, &prepared.files)
            .map_err(super::portability::bundle_app_error)?;
        cache.insert(
            package.clone(),
            CachedCompiledPackage {
                generation,
                compiled: Arc::new(prepared.compiled),
            },
        );
        let summary = self
            .summary(&package)
            .map_err(|error| protocol_package_app_error(&error))?;
        let summary = summary.ok_or_else(|| {
            protocol_package_app_error(&ProtocolPackageStorageError::NotFound {
                package: package.clone(),
            })
        })?;
        Ok(ProtocolPackageImportViewModel {
            outcome: ProtocolPackageImportOutcomeViewModel::Installed,
            version: application_summary(summary),
            capabilities: description.capabilities,
            schema: description.schema,
        })
    }
}

pub(super) fn builtin_header(prepared: &PreparedProtocolPackage) -> StoredProtocolPackageHeader {
    let manifest = prepared.compiled.manifest();
    StoredProtocolPackageHeader {
        package: prepared.compiled.package().clone(),
        name: manifest.package().name().to_owned(),
        host_api: manifest.api(),
        enabled: true,
        validation: StoredProtocolPackageValidation::Valid,
        installed_at: Utc::now(),
        generation: Uuid::new_v4(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Cursor, Write},
        path::{Path, PathBuf},
        sync::Arc,
    };

    use intercept_proxy_application::BuiltinProtocolPackagePort;
    use serde_json::json;
    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::{SqliteStore, WorkspaceRecord};

    fn harness() -> (TempDir, Arc<SqliteStore>, Vec<u8>) {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(SqliteStore::open(&directory.path().join("state.db")).unwrap());
        (directory, store, template_zip(None))
    }

    fn repository(store: Arc<SqliteStore>, archive: Vec<u8>) -> ProtocolPackageRepositoryAdapter {
        ProtocolPackageRepositoryAdapter::with_default_limits(store)
            .with_builtin_archive(Arc::from(archive))
    }

    #[test]
    fn fresh_and_repeated_seed_produce_one_enabled_official_version() {
        let (_directory, store, archive) = harness();
        let repository = repository(store, archive);
        repository.ensure_builtin_seeded().unwrap();
        repository.ensure_builtin_seeded().unwrap();

        let versions = repository.list().unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].package, builtin_iso8583_package_ref());
        assert!(versions[0].enabled);
        repository.revalidate(&versions[0].package).unwrap();
    }

    #[test]
    fn deleted_package_is_not_silently_recreated_on_restart_seed() {
        let (_directory, store, archive) = harness();
        let repository = repository(store, archive);
        repository.ensure_builtin_seeded().unwrap();
        repository.delete(&builtin_iso8583_package_ref()).unwrap();

        repository.ensure_builtin_seeded().unwrap();
        assert!(repository.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn explicit_restore_repairs_corruption_and_reset_restores_deletion() {
        let (_directory, store, archive) = harness();
        let repository = repository(Arc::clone(&store), archive);
        repository.ensure_builtin_seeded().unwrap();
        store
            .execute_test_batch(
                "UPDATE protocol_package_files SET contents = X'00'
                 WHERE package_id = 'iso8583-ascii-standard' AND path = 'protocol.rhai';",
            )
            .unwrap();
        assert!(
            repository
                .revalidate(&builtin_iso8583_package_ref())
                .is_err()
        );
        repository.ensure_builtin_seeded().unwrap();
        assert!(
            repository
                .revalidate(&builtin_iso8583_package_ref())
                .is_err()
        );

        repository.restore_builtin().await.unwrap();
        repository
            .revalidate(&builtin_iso8583_package_ref())
            .unwrap();
        repository.delete(&builtin_iso8583_package_ref()).unwrap();
        let workspace_id = uuid::Uuid::new_v4();
        repository
            .reset_with_builtin(
                workspace_id,
                &[WorkspaceRecord {
                    id: workspace_id,
                    revision: 0,
                    value: json!({"format_version": 5}),
                    updated_at: chrono::Utc::now(),
                }],
                &json!({}),
            )
            .unwrap();
        let restored = repository.list().unwrap();
        assert_eq!(restored.len(), 1);
        assert!(restored[0].enabled);
    }

    #[test]
    fn first_seed_rejects_same_identity_with_different_content() {
        let (_directory, store, official) = harness();
        let foreign = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
        foreign
            .install_zip(&template_zip(Some(b"\nchanged")))
            .unwrap();

        let error = repository(store, official)
            .ensure_builtin_seeded()
            .unwrap_err();
        assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_IDENTITY_CONFLICT");
    }

    #[tokio::test]
    async fn failed_restore_keeps_existing_database_generation_and_cache() {
        let (_directory, store, archive) = harness();
        let repository = repository(Arc::clone(&store), archive);
        repository.ensure_builtin_seeded().unwrap();
        let package = builtin_iso8583_package_ref();
        let cached = repository.compiled(&package).unwrap();
        store
            .execute_test_batch("DROP TABLE application_feature_state;")
            .unwrap();

        assert!(repository.restore_builtin().await.is_err());
        let after = repository.compiled(&package).unwrap();
        assert!(Arc::ptr_eq(&cached, &after));
    }

    fn template_zip(readme_suffix: Option<&[u8]>) -> Vec<u8> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../templates/socket-protocol/iso8583-standard");
        let mut paths = Vec::new();
        collect_files(&root, &root, &mut paths);
        paths.sort();
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for path in paths {
            let relative = path.strip_prefix(&root).unwrap().to_string_lossy();
            writer
                .start_file(relative.replace('\\', "/"), SimpleFileOptions::default())
                .unwrap();
            let mut bytes = fs::read(&path).unwrap();
            if path.ends_with("README.md")
                && !path.ends_with("samples/README.md")
                && let Some(suffix) = readme_suffix
            {
                bytes.extend_from_slice(suffix);
            }
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn collect_files(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
        assert!(directory.starts_with(root));
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_files(root, &path, files);
            } else {
                files.push(path);
            }
        }
    }
}
