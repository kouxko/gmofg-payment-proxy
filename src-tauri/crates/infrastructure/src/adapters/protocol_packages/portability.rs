//! 可移植协议包的恢复、导出与组合事务端口。

use std::collections::HashSet;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, ApplicationBackupProtocolPackageBaseline,
    ApplicationConfigurationDocument, MAX_PORTABLE_PACKAGE_FILE_BYTES, MAX_PORTABLE_PACKAGE_FILES,
    MAX_PORTABLE_PACKAGE_TOTAL_BYTES, MAX_PORTABLE_PROTOCOL_PACKAGES,
    PortableApplicationProtocolPackage, PortableProtocolPackageFile,
    ProtocolPackageDescriptionViewModel, ProtocolPackagePortabilityPort,
    validate_portable_protocol_bindings,
};
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_protocol_scripting::{ProtocolPackageFiles, restore_protocol_package_files};
use uuid::Uuid;

use crate::{
    WorkspaceRecord,
    adapters::{settings::serialize_settings, workspaces::WorkspaceRepositoryAdapter},
    sqlite::protocol_packages::{
        StoredProtocolPackageBundleError, StoredProtocolPackageFiles, StoredProtocolPackageHeader,
        StoredProtocolPackageValidation, StoredProtocolPackageWrite,
    },
};

use super::{
    PreparedProtocolPackage, ProtocolPackageRepositoryAdapter, ProtocolPackageStorageError,
    application_descriptions, protocol_package_app_error,
};

#[async_trait]
impl ProtocolPackagePortabilityPort for ProtocolPackageRepositoryAdapter {
    async fn application_backup_baseline(
        &self,
    ) -> AppResult<Vec<ApplicationBackupProtocolPackageBaseline>> {
        self.store
            .list_protocol_package_headers()
            .map(|headers| {
                headers
                    .into_iter()
                    .map(|header| ApplicationBackupProtocolPackageBaseline {
                        package: header.package,
                        enabled: header.enabled,
                        generation: header.generation,
                    })
                    .collect()
            })
            .map_err(ProtocolPackageStorageError::from)
            .map_err(|error| protocol_package_app_error(&error))
    }

    async fn export_application_packages(
        &self,
    ) -> AppResult<Vec<PortableApplicationProtocolPackage>> {
        let identities = self
            .store
            .list_protocol_package_headers()
            .map_err(ProtocolPackageStorageError::from)
            .map_err(|error| protocol_package_app_error(&error))?;
        let mut exported = identities
            .into_iter()
            .map(|header| {
                let (files, enabled) = self.export_one(&header.package)?;
                Ok(PortableApplicationProtocolPackage {
                    package: header.package,
                    files,
                    enabled,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        sort_application_packages(&mut exported);
        Ok(exported)
    }

    async fn preflight_application_packages(
        &self,
        packages: &[PortableApplicationProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        self.prepare_application_packages(packages)
            .map(|prepared| application_descriptions(&prepared))
            .map_err(|error| protocol_package_app_error(&error))
    }

    async fn preflight_installed_packages(
        &self,
        packages: &[ProtocolPackageRef],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        require_unique_identities(packages)?;
        installed_references::prepare_installed_references(self, packages.to_vec())
            .map(|prepared| application_descriptions(&prepared))
            .map_err(|error| protocol_package_app_error(&error))
    }

    async fn replace_application_bundle(
        &self,
        packages: Vec<PortableApplicationProtocolPackage>,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        document.validate()?;
        if packages != document.protocol_packages {
            return Err(AppError::new(
                "PORTABLE_PROTOCOL_PACKAGE_INVALID",
                "提交的协议包集合必须与完整配置文档完全一致。",
            ));
        }
        let expected = packages
            .iter()
            .map(|package| package.package.clone())
            .collect::<Vec<_>>();
        let enabled = packages
            .iter()
            .map(|package| package.enabled)
            .collect::<Vec<_>>();
        let prepared = self
            .prepare_application_packages(&packages)
            .map_err(|error| protocol_package_app_error(&error))?;
        let descriptions = application_descriptions(&prepared);
        validate_portable_protocol_bindings(&document.workspaces, &expected, &descriptions)?;
        let writes = prepared
            .into_iter()
            .zip(enabled)
            .map(|(prepared, enabled)| prepared_into_write(prepared, enabled))
            .collect::<Vec<_>>();
        let (records, settings) = application_records(&document)?;
        let mut cache = self.cache.lock();
        self.store
            .replace_application_bundle(
                document.selected_workspace_id.as_uuid(),
                &records,
                &settings,
                &writes,
            )
            .map_err(bundle_app_error)?;
        cache.clear();
        Ok(())
    }

    async fn reset_application_bundle(
        &self,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        document.validate()?;
        if !document.protocol_packages.is_empty() {
            return Err(AppError::new(
                "PORTABLE_PROTOCOL_PACKAGE_INVALID",
                "重置后的默认配置不能携带协议包。",
            ));
        }
        let (records, settings) = application_records(&document)?;
        self.reset_with_builtin(
            document.selected_workspace_id.as_uuid(),
            &records,
            &settings,
        )
    }
}

impl ProtocolPackageRepositoryAdapter {
    fn export_one(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<(Vec<PortableProtocolPackageFile>, bool)> {
        // 保持注册表的全局锁顺序：先 cache，再进入 SqliteStore。导出不会修改缓存。
        let _cache = self.cache.lock();
        let stored = self
            .store
            .load_protocol_package(package)
            .map_err(ProtocolPackageStorageError::from)
            .map_err(|error| protocol_package_app_error(&error))?
            .ok_or_else(|| {
                protocol_package_app_error(&ProtocolPackageStorageError::NotFound {
                    package: package.clone(),
                })
            })?;
        let rows = match stored.files {
            StoredProtocolPackageFiles::Valid(rows) => rows,
            StoredProtocolPackageFiles::Rejected(code) => {
                return Err(protocol_package_app_error(
                    &ProtocolPackageStorageError::StoredPackageInvalid {
                        package: package.clone(),
                        code: code.to_owned(),
                    },
                ));
            }
        };
        // 数据库始终按不可信输入恢复并重新编译；导出绝不传播损坏或身份错配的内容。
        let prepared = self
            .prepare_rows(package, rows)
            .map_err(|error| protocol_package_app_error(&error))?;
        Ok((portable_files(&prepared.files), stored.header.enabled))
    }

    fn prepare_application_packages(
        &self,
        packages: &[PortableApplicationProtocolPackage],
    ) -> Result<Vec<PreparedProtocolPackage>, ProtocolPackageStorageError> {
        require_package_count(packages.len())?;
        require_unique_identities_storage(packages.iter().map(|package| &package.package))?;
        packages
            .iter()
            .map(|package| self.prepare_rows(&package.package, decode_files(&package.files)?))
            .collect()
    }

    fn prepare_rows(
        &self,
        expected: &ProtocolPackageRef,
        rows: Vec<(String, Vec<u8>)>,
    ) -> Result<PreparedProtocolPackage, ProtocolPackageStorageError> {
        let files = restore_protocol_package_files(rows, &self.archive_limits)
            .map_err(|source| ProtocolPackageStorageError::Archive { source })?;
        let compiled = self
            .compiler
            .compile(&files)
            .map_err(|source| ProtocolPackageStorageError::Compilation { source })?;
        if compiled.package() != expected {
            return Err(ProtocolPackageStorageError::PortableInvalid);
        }
        Ok(PreparedProtocolPackage { files, compiled })
    }
}

fn application_records(
    document: &ApplicationConfigurationDocument,
) -> AppResult<(Vec<WorkspaceRecord>, serde_json::Value)> {
    let records = document
        .workspaces
        .iter()
        .map(WorkspaceRepositoryAdapter::record)
        .collect::<AppResult<Vec<_>>>()?;
    let settings = serialize_settings(&document.settings.to_draft(None)).map_err(|error| {
        AppError::new(
            "APPLICATION_CONFIGURATION_INVALID",
            format!("完整配置中的 Settings 无法持久化：{error}"),
        )
    })?;
    Ok((records, settings))
}

fn prepared_into_write(
    prepared: PreparedProtocolPackage,
    enabled: bool,
) -> StoredProtocolPackageWrite {
    let manifest = prepared.compiled.manifest();
    let header = StoredProtocolPackageHeader {
        package: prepared.compiled.package().clone(),
        name: manifest.package().name().to_owned(),
        host_api: manifest.api(),
        enabled,
        validation: StoredProtocolPackageValidation::Valid,
        installed_at: Utc::now(),
        generation: Uuid::new_v4(),
    };
    StoredProtocolPackageWrite {
        header,
        files: prepared.files,
    }
}

fn decode_files(
    files: &[PortableProtocolPackageFile],
) -> Result<Vec<(String, Vec<u8>)>, ProtocolPackageStorageError> {
    if files.is_empty() || files.len() > MAX_PORTABLE_PACKAGE_FILES {
        return Err(ProtocolPackageStorageError::PortableInvalid);
    }
    let mut total = 0_usize;
    files
        .iter()
        .map(|file| {
            let max_encoded = MAX_PORTABLE_PACKAGE_FILE_BYTES.div_ceil(3) * 4;
            if file.contents_base64.len() > max_encoded {
                return Err(ProtocolPackageStorageError::PortableInvalid);
            }
            let bytes = STANDARD
                .decode(&file.contents_base64)
                .map_err(|_| ProtocolPackageStorageError::PortableInvalid)?;
            if STANDARD.encode(&bytes) != file.contents_base64 {
                return Err(ProtocolPackageStorageError::PortableInvalid);
            }
            if bytes.len() > MAX_PORTABLE_PACKAGE_FILE_BYTES {
                return Err(ProtocolPackageStorageError::PortableInvalid);
            }
            total = total
                .checked_add(bytes.len())
                .ok_or(ProtocolPackageStorageError::PortableInvalid)?;
            if total > MAX_PORTABLE_PACKAGE_TOTAL_BYTES {
                return Err(ProtocolPackageStorageError::PortableInvalid);
            }
            Ok((file.path.clone(), bytes))
        })
        .collect()
}

fn portable_files(files: &ProtocolPackageFiles) -> Vec<PortableProtocolPackageFile> {
    files
        .iter()
        .map(|(path, bytes)| PortableProtocolPackageFile {
            path: path.as_str().to_owned(),
            contents_base64: STANDARD.encode(bytes),
        })
        .collect()
}

fn require_unique_identities(packages: &[ProtocolPackageRef]) -> AppResult<()> {
    require_package_count(packages.len()).map_err(|error| protocol_package_app_error(&error))?;
    require_unique_identities_storage(packages.iter())
        .map_err(|error| protocol_package_app_error(&error))
}

fn sort_application_packages(packages: &mut [PortableApplicationProtocolPackage]) {
    packages.sort_by(|left, right| compare_identities(&left.package, &right.package));
}

fn compare_identities(left: &ProtocolPackageRef, right: &ProtocolPackageRef) -> std::cmp::Ordering {
    left.id.as_str().cmp(right.id.as_str()).then_with(|| {
        left.version
            .semantic_cmp(&right.version)
            .then_with(|| left.version.as_str().cmp(right.version.as_str()))
    })
}

fn require_unique_identities_storage<'a>(
    packages: impl IntoIterator<Item = &'a ProtocolPackageRef>,
) -> Result<(), ProtocolPackageStorageError> {
    let mut identities = HashSet::new();
    for package in packages {
        if !identities.insert(package.clone()) {
            return Err(ProtocolPackageStorageError::PortableInvalid);
        }
    }
    Ok(())
}

fn require_package_count(count: usize) -> Result<(), ProtocolPackageStorageError> {
    if count > MAX_PORTABLE_PROTOCOL_PACKAGES {
        Err(ProtocolPackageStorageError::PortableInvalid)
    } else {
        Ok(())
    }
}

pub(super) fn bundle_app_error(error: StoredProtocolPackageBundleError) -> AppError {
    match error {
        StoredProtocolPackageBundleError::IdentityConflict(package) => {
            protocol_package_app_error(&ProtocolPackageStorageError::IdentityConflict { package })
        }
        StoredProtocolPackageBundleError::Infrastructure(error) => {
            protocol_package_app_error(&ProtocolPackageStorageError::Infrastructure(error))
        }
    }
}

#[path = "portability/installed_references.rs"]
mod installed_references;

#[cfg(test)]
#[path = "portability_tests.rs"]
mod tests;
