//! 可重建编译缓存及其持久化代际校验。

use std::{collections::HashMap, sync::Arc};

use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_protocol_scripting::{CompiledProtocolPackage, restore_protocol_package_files};
use uuid::Uuid;

use super::error::compilation_code;
#[cfg(test)]
use super::{ProtocolPackageRecoveryFailure, ProtocolPackageRecoveryReport};
use super::{ProtocolPackageRepositoryAdapter, ProtocolPackageStorageError};
use crate::sqlite::protocol_packages::{
    StoredProtocolPackageFiles, StoredProtocolPackageValidation,
};

#[derive(Debug)]
pub(super) struct CachedCompiledPackage {
    pub(super) generation: Uuid,
    pub(super) compiled: Arc<CompiledProtocolPackage>,
}

impl CachedCompiledPackage {
    pub(super) fn matches(&self, generation: Uuid, package: &ProtocolPackageRef) -> bool {
        self.generation == generation && self.compiled.package() == package
    }
}

impl ProtocolPackageRepositoryAdapter {
    pub(super) async fn revalidate_async(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Arc<CompiledProtocolPackage>, ProtocolPackageStorageError> {
        let selected = package.clone();
        let cache = Arc::clone(&self.cache);
        let archive_limits = self.archive_limits.clone();
        let compiler = self.compiler;
        #[cfg(test)]
        let revalidate_after_load_hook = Arc::clone(&self.revalidate_after_load_hook);
        self.executor
            .execute(move |store| {
                let mut cache = cache.lock();
                cache.remove(&selected);
                Self::revalidate_on_store(
                    store,
                    &selected,
                    &archive_limits,
                    compiler,
                    &mut cache,
                    #[cfg(test)]
                    &revalidate_after_load_hook,
                )
            })
            .await
    }

    fn revalidate_on_store(
        store: &crate::SqliteStore,
        package: &ProtocolPackageRef,
        archive_limits: &intercept_proxy_protocol_scripting::ProtocolArchiveLimits,
        package_compiler: intercept_proxy_protocol_scripting::ProtocolPackageCompiler,
        cache: &mut HashMap<ProtocolPackageRef, CachedCompiledPackage>,
        #[cfg(test)] revalidate_after_load_hook: &Arc<
            parking_lot::Mutex<Option<super::RevalidateAfterLoadHook>>,
        >,
    ) -> Result<Arc<CompiledProtocolPackage>, ProtocolPackageStorageError> {
        let stored = store.load_protocol_package(package)?.ok_or_else(|| {
            ProtocolPackageStorageError::NotFound {
                package: package.clone(),
            }
        })?;
        #[cfg(test)]
        if let Some(hook) = revalidate_after_load_hook.lock().take() {
            let _ = hook.entered.send(());
            hook.release
                .recv()
                .expect("revalidation test releases loaded package");
        }
        if matches!(
            stored.header.validation,
            StoredProtocolPackageValidation::Invalid(ref code) if code == "PERSISTENCE_CORRUPT"
        ) {
            return Self::reject_stored_on(store, package, "PERSISTENCE_CORRUPT", cache);
        }
        let stored_files = match stored.files {
            StoredProtocolPackageFiles::Valid(files) => files,
            StoredProtocolPackageFiles::Rejected(code) => {
                return Self::reject_stored_on(store, package, code, cache);
            }
        };
        let files = match restore_protocol_package_files(stored_files, archive_limits) {
            Ok(files) => files,
            Err(error) => {
                return Self::reject_stored_on(store, package, error.code().as_str(), cache);
            }
        };
        let compiled_package = match package_compiler.compile(&files) {
            Ok(package) => package,
            Err(error) => {
                return Self::reject_stored_on(store, package, compilation_code(&error), cache);
            }
        };
        if compiled_package.package() != package
            || compiled_package.manifest().package().name() != stored.header.name
            || compiled_package.manifest().api() != stored.header.host_api
            || compiled_package.kind() != stored.header.kind
        {
            return Self::reject_stored_on(store, package, "STORED_IDENTITY_MISMATCH", cache);
        }
        if !store.set_protocol_package_validation(package, None)? {
            return Err(ProtocolPackageStorageError::NotFound {
                package: package.clone(),
            });
        }
        let compiled = Arc::new(compiled_package);
        cache.insert(
            package.clone(),
            CachedCompiledPackage {
                generation: stored.header.generation,
                compiled: Arc::clone(&compiled),
            },
        );
        Ok(compiled)
    }

    fn reject_stored_on<T>(
        store: &crate::SqliteStore,
        package: &ProtocolPackageRef,
        code: &str,
        cache: &mut HashMap<ProtocolPackageRef, CachedCompiledPackage>,
    ) -> Result<T, ProtocolPackageStorageError> {
        cache.remove(package);
        if !store.set_protocol_package_validation(package, Some(code))? {
            return Err(ProtocolPackageStorageError::NotFound {
                package: package.clone(),
            });
        }
        Err(ProtocolPackageStorageError::StoredPackageInvalid {
            package: package.clone(),
            code: code.to_owned(),
        })
    }

    #[cfg(test)]
    pub(super) fn pause_next_revalidate_after_load(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        *self.revalidate_after_load_hook.lock() = Some(super::RevalidateAfterLoadHook {
            entered: entered_tx,
            release: release_rx,
        });
        (entered_rx, release_tx)
    }

    /// 获取可执行编译产物。缓存缺失时重新验证数据库路径、文件限制、Manifest、Schema 和 Rhai。
    #[cfg(test)]
    pub fn compiled(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Arc<CompiledProtocolPackage>, ProtocolPackageStorageError> {
        let mut cache = self.cache.lock();
        self.compiled_locked(package, &mut cache)
    }

    /// 忽略现有 AST 缓存，对持久化文件执行完整恢复、身份校验和 Rhai 重新编译。
    #[cfg(test)]
    pub fn revalidate(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Arc<CompiledProtocolPackage>, ProtocolPackageStorageError> {
        let mut cache = self.cache.lock();
        cache.remove(package);
        self.compiled_locked(package, &mut cache)
    }

    #[cfg(test)]
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
    #[cfg(test)]
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

    #[cfg(test)]
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

    #[cfg(test)]
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
