//! 可重建编译缓存及其持久化代际校验。

use std::{collections::HashMap, sync::Arc};

use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_protocol_scripting::{CompiledProtocolPackage, restore_protocol_package_files};
use uuid::Uuid;

use super::error::compilation_code;
use super::{
    ProtocolPackageRecoveryFailure, ProtocolPackageRecoveryReport,
    ProtocolPackageRepositoryAdapter, ProtocolPackageStorageError,
};
use crate::sqlite::protocol_packages::{
    StoredProtocolPackageFiles, StoredProtocolPackageValidation,
};

#[derive(Debug)]
pub(super) struct CachedCompiledPackage {
    pub(super) generation: Uuid,
    pub(super) compiled: Arc<CompiledProtocolPackage>,
}

impl ProtocolPackageRepositoryAdapter {
    /// 获取可执行编译产物。缓存缺失时重新验证数据库路径、文件限制、Manifest、Schema 和 Rhai。
    pub fn compiled(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Arc<CompiledProtocolPackage>, ProtocolPackageStorageError> {
        let mut cache = self.cache.lock();
        self.compiled_locked(package, &mut cache)
    }

    /// 忽略现有 AST 缓存，对持久化文件执行完整恢复、身份校验和 Rhai 重新编译。
    pub fn revalidate(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Arc<CompiledProtocolPackage>, ProtocolPackageStorageError> {
        let mut cache = self.cache.lock();
        cache.remove(package);
        self.compiled_locked(package, &mut cache)
    }

    fn compiled_locked(
        &self,
        package: &ProtocolPackageRef,
        cache: &mut HashMap<ProtocolPackageRef, CachedCompiledPackage>,
    ) -> Result<Arc<CompiledProtocolPackage>, ProtocolPackageStorageError> {
        if let Some((cached_generation, cached_compiled)) = cache
            .get(package)
            .map(|cached| (cached.generation, Arc::clone(&cached.compiled)))
        {
            let Some(header) = self.store.load_protocol_package_header(package)? else {
                cache.remove(package);
                return Err(ProtocolPackageStorageError::NotFound {
                    package: package.clone(),
                });
            };
            if let StoredProtocolPackageValidation::Invalid(code) = header.validation {
                return self.reject_stored(package, &code, cache);
            }
            if header.generation == cached_generation {
                return Ok(cached_compiled);
            }
            cache.remove(package);
        }
        let stored = self.store.load_protocol_package(package)?.ok_or_else(|| {
            ProtocolPackageStorageError::NotFound {
                package: package.clone(),
            }
        })?;
        if matches!(
            stored.header.validation,
            StoredProtocolPackageValidation::Invalid(ref code) if code == "PERSISTENCE_CORRUPT"
        ) {
            return self.reject_stored(package, "PERSISTENCE_CORRUPT", cache);
        }
        let stored_files = match stored.files {
            StoredProtocolPackageFiles::Valid(files) => files,
            StoredProtocolPackageFiles::Rejected(code) => {
                return self.reject_stored(package, code, cache);
            }
        };
        let files = match restore_protocol_package_files(stored_files, &self.archive_limits) {
            Ok(files) => files,
            Err(error) => return self.reject_stored(package, error.code().as_str(), cache),
        };
        let compiled = match self.compiler.compile(&files) {
            Ok(compiled) => compiled,
            Err(error) => return self.reject_stored(package, compilation_code(&error), cache),
        };
        if compiled.package() != package
            || compiled.manifest().package().name() != stored.header.name
            || compiled.manifest().api() != stored.header.host_api
            || compiled.kind() != stored.header.kind
        {
            return self.reject_stored(package, "STORED_IDENTITY_MISMATCH", cache);
        }
        self.require_validation_update(package, None)?;
        let compiled = Arc::new(compiled);
        cache.insert(
            package.clone(),
            CachedCompiledPackage {
                generation: stored.header.generation,
                compiled: Arc::clone(&compiled),
            },
        );
        Ok(compiled)
    }

    /// 清空派生缓存并逐个重新编译。坏包会持久化为 Invalid，其他版本仍可恢复。
    pub fn recover_cache(
        &self,
    ) -> Result<ProtocolPackageRecoveryReport, ProtocolPackageStorageError> {
        let mut cache = self.cache.lock();
        cache.clear();
        let packages = self
            .store
            .list_protocol_package_headers()?
            .into_iter()
            .map(|header| header.package)
            .collect::<Vec<_>>();
        let mut report = ProtocolPackageRecoveryReport::default();
        for package in packages {
            match self.compiled_locked(&package, &mut cache) {
                Ok(_) => report.loaded.push(package),
                Err(ProtocolPackageStorageError::StoredPackageInvalid { code, .. }) => {
                    report
                        .failed
                        .push(ProtocolPackageRecoveryFailure { package, code });
                }
                Err(error) => return Err(error),
            }
        }
        Ok(report)
    }

    fn reject_stored<T>(
        &self,
        package: &ProtocolPackageRef,
        code: &str,
        cache: &mut HashMap<ProtocolPackageRef, CachedCompiledPackage>,
    ) -> Result<T, ProtocolPackageStorageError> {
        cache.remove(package);
        self.require_validation_update(package, Some(code))?;
        Err(ProtocolPackageStorageError::StoredPackageInvalid {
            package: package.clone(),
            code: code.to_owned(),
        })
    }

    pub(super) fn require_validation_update(
        &self,
        package: &ProtocolPackageRef,
        error_code: Option<&str>,
    ) -> Result<(), ProtocolPackageStorageError> {
        if self
            .store
            .set_protocol_package_validation(package, error_code)?
        {
            Ok(())
        } else {
            Err(ProtocolPackageStorageError::NotFound {
                package: package.clone(),
            })
        }
    }
}
