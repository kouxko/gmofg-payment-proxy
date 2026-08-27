//! 应用级协议包注册表适配器。
//!
//! 导入在事务外完成 ZIP 安全读取和 Rhai 编译；数据库只保存规范文件，缓存可由相同校验链重建。

use std::{collections::HashMap, io::Cursor, sync::Arc};

use chrono::Utc;
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_protocol_scripting::{
    ProtocolArchiveLimits, ProtocolPackageCompiler, ProtocolRuntimeLimits,
    read_protocol_package_zip,
};
use parking_lot::Mutex;
use uuid::Uuid;

const MAX_RUNTIME_PACKAGE_COMPILATIONS: usize = 4;

#[cfg(test)]
use crate::SqliteStore;
use crate::{IntoSqlitePersistence, SqliteExecutor};

use super::super::sqlite::protocol_packages::{
    StoredProtocolPackageHeader, StoredProtocolPackageInstallOutcome,
    StoredProtocolPackageValidation,
};

/// `SQLite` 持久化与可重建 Rhai 编译缓存的组合适配器。
#[derive(Debug)]
pub struct ProtocolPackageRepositoryAdapter {
    #[cfg(test)]
    store: Arc<SqliteStore>,
    executor: SqliteExecutor,
    archive_limits: ProtocolArchiveLimits,
    runtime_limits: ProtocolRuntimeLimits,
    compiler: ProtocolPackageCompiler,
    runtime_compile_gate: Arc<tokio::sync::Semaphore>,
    builtin_archive: Option<Arc<[u8]>>,
    cache: Arc<Mutex<HashMap<ProtocolPackageRef, CachedCompiledPackage>>>,
    #[cfg(test)]
    revalidate_after_load_hook: Arc<Mutex<Option<RevalidateAfterLoadHook>>>,
}

#[cfg(test)]
#[derive(Debug)]
struct RevalidateAfterLoadHook {
    entered: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

impl ProtocolPackageRepositoryAdapter {
    /// 使用显式资源限制创建注册表；测试和未来配置入口可收紧默认值。
    #[must_use]
    pub fn new(
        persistence: impl IntoSqlitePersistence,
        archive_limits: ProtocolArchiveLimits,
        runtime_limits: ProtocolRuntimeLimits,
    ) -> Self {
        let (executor, store) = persistence.into_sqlite_persistence();
        #[cfg(not(test))]
        drop(store);
        Self {
            #[cfg(test)]
            store,
            executor,
            archive_limits,
            runtime_limits,
            compiler: ProtocolPackageCompiler::new(runtime_limits),
            runtime_compile_gate: Arc::new(tokio::sync::Semaphore::new(
                MAX_RUNTIME_PACKAGE_COMPILATIONS,
            )),
            builtin_archive: None,
            cache: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            revalidate_after_load_hook: Arc::new(Mutex::new(None)),
        }
    }

    #[must_use]
    pub fn with_default_limits(persistence: impl IntoSqlitePersistence) -> Self {
        Self::new(
            persistence,
            ProtocolArchiveLimits::default(),
            ProtocolRuntimeLimits::default(),
        )
    }

    /// 完整校验 ZIP 后原子安装。新包默认停用；相同内容重入不改变已有启用状态和安装时间。
    #[cfg(test)]
    pub fn install_zip(
        &self,
        zip_bytes: &[u8],
    ) -> Result<ProtocolPackageInstallOutcome, ProtocolPackageStorageError> {
        self.install_prepared(self.prepare_zip(zip_bytes)?)
    }

    /// 完整校验 ZIP 并返回尚未持久化的准备对象。
    ///
    /// 本方法不写数据库、不改启用位，也不插入运行时缓存。调用方可先生成预览并等待
    /// 用户确认；最终提交始终使用这里冻结的规范文件，原 ZIP 后续被替换也没有影响。
    pub(crate) fn prepare_zip(
        &self,
        zip_bytes: &[u8],
    ) -> Result<PreparedProtocolPackage, ProtocolPackageStorageError> {
        let files = read_protocol_package_zip(Cursor::new(zip_bytes), &self.archive_limits)
            .map_err(|source| ProtocolPackageStorageError::Archive { source })?;
        let compiled = self
            .compiler
            .compile(&files)
            .map_err(|source| ProtocolPackageStorageError::Compilation { source })?;
        Ok(PreparedProtocolPackage { files, compiled })
    }

    /// 原子提交一个已经完整验证的准备对象。
    ///
    /// `SQLite` 事务仍重新判断身份冲突与幂等复用，防止 prepare 到 commit 之间的并发安装
    /// 覆盖现有版本；提交不会重新读取或重新编译原 ZIP。
    #[cfg(test)]
    pub(crate) fn install_prepared(
        &self,
        prepared: PreparedProtocolPackage,
    ) -> Result<ProtocolPackageInstallOutcome, ProtocolPackageStorageError> {
        let PreparedProtocolPackage { files, compiled } = prepared;
        let manifest = compiled.manifest();
        let header = StoredProtocolPackageHeader {
            package: compiled.package().clone(),
            name: manifest.package().name().to_owned(),
            host_api: manifest.api(),
            kind: compiled.kind(),
            enabled: false,
            validation: StoredProtocolPackageValidation::Valid,
            installed_at: Utc::now(),
            generation: Uuid::new_v4(),
        };
        // 该锁不仅保护 HashMap，也把“数据库身份是否存在”与缓存增删串成一个注册表操作。
        // 因而同一适配器上的 install/compile/delete 不会留下数据库已删、缓存却随后插入的幽灵包。
        let mut cache = self.cache.lock();
        let outcome = self.store.install_protocol_package(&header, &files)?;
        let (installed, generation) = match outcome {
            StoredProtocolPackageInstallOutcome::IdentityConflict => {
                return Err(ProtocolPackageStorageError::IdentityConflict {
                    package: header.package,
                });
            }
            StoredProtocolPackageInstallOutcome::Installed(generation) => (true, generation),
            StoredProtocolPackageInstallOutcome::Reused(generation) => (false, generation),
        };
        self.require_validation_update(&header.package, None)?;
        cache.insert(
            header.package.clone(),
            CachedCompiledPackage {
                generation,
                compiled: Arc::new(compiled),
            },
        );
        let summary = self.summary(&header.package)?.ok_or_else(|| {
            ProtocolPackageStorageError::NotFound {
                package: header.package.clone(),
            }
        })?;
        Ok(if installed {
            ProtocolPackageInstallOutcome::Installed(summary)
        } else {
            ProtocolPackageInstallOutcome::Reused(summary)
        })
    }

    pub(crate) async fn install_prepared_async(
        &self,
        prepared: PreparedProtocolPackage,
    ) -> Result<ProtocolPackageInstallOutcome, ProtocolPackageStorageError> {
        let PreparedProtocolPackage { files, compiled } = prepared;
        let manifest = compiled.manifest();
        let header = StoredProtocolPackageHeader {
            package: compiled.package().clone(),
            name: manifest.package().name().to_owned(),
            host_api: manifest.api(),
            kind: compiled.kind(),
            enabled: false,
            validation: StoredProtocolPackageValidation::Valid,
            installed_at: Utc::now(),
            generation: Uuid::new_v4(),
        };
        let cache = Arc::clone(&self.cache);
        self.executor
            .execute(move |store| {
                let mut cache = cache.lock();
                let outcome = store.install_protocol_package(&header, &files)?;
                let (installed, generation) = match outcome {
                    StoredProtocolPackageInstallOutcome::IdentityConflict => {
                        return Err(ProtocolPackageStorageError::IdentityConflict {
                            package: header.package,
                        });
                    }
                    StoredProtocolPackageInstallOutcome::Installed(generation) => {
                        (true, generation)
                    }
                    StoredProtocolPackageInstallOutcome::Reused(generation) => (false, generation),
                };
                if !store.set_protocol_package_validation(&header.package, None)? {
                    return Err(ProtocolPackageStorageError::NotFound {
                        package: header.package,
                    });
                }
                cache.insert(
                    header.package.clone(),
                    CachedCompiledPackage {
                        generation,
                        compiled: Arc::new(compiled),
                    },
                );
                debug_assert!(
                    cache
                        .get(&header.package)
                        .is_some_and(|cached| cached.matches(generation, &header.package))
                );
                let summary = store
                    .load_protocol_package_header(&header.package)?
                    .map(summary_from_header)
                    .ok_or_else(|| ProtocolPackageStorageError::NotFound {
                        package: header.package.clone(),
                    })?;
                Ok(if installed {
                    ProtocolPackageInstallOutcome::Installed(summary)
                } else {
                    ProtocolPackageInstallOutcome::Reused(summary)
                })
            })
            .await
    }

    /// 按 ID、版本排序列出无源码记录。该操作只反映最近一次恢复状态，不隐式执行脚本编译。
    #[cfg(test)]
    pub fn list(&self) -> Result<Vec<ProtocolPackageSummary>, ProtocolPackageStorageError> {
        Ok(self
            .store
            .list_protocol_package_headers()?
            .into_iter()
            .map(summary_from_header)
            .collect())
    }

    /// 读取一个无源码精确版本记录。
    #[cfg(test)]
    pub fn summary(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Option<ProtocolPackageSummary>, ProtocolPackageStorageError> {
        Ok(self
            .store
            .load_protocol_package_header(package)?
            .map(summary_from_header))
    }

    /// 只持久化启用位；能否启停以及引用约束由 T14 Application 用例决定。
    #[cfg(test)]
    pub fn set_enabled(
        &self,
        package: &ProtocolPackageRef,
        enabled: bool,
    ) -> Result<(), ProtocolPackageStorageError> {
        if self.store.set_protocol_package_enabled(package, enabled)? {
            Ok(())
        } else {
            Err(ProtocolPackageStorageError::NotFound {
                package: package.clone(),
            })
        }
    }

    /// 删除精确版本及其级联文件；引用约束由 T14 在调用此存储原语之前判断。
    #[cfg(test)]
    pub fn delete(&self, package: &ProtocolPackageRef) -> Result<(), ProtocolPackageStorageError> {
        let mut cache = self.cache.lock();
        if self.store.delete_protocol_package(package)? {
            cache.remove(package);
            Ok(())
        } else {
            Err(ProtocolPackageStorageError::NotFound {
                package: package.clone(),
            })
        }
    }
}

mod application_port;
pub(super) use application_port::{
    application_description, application_descriptions, application_summary,
    protocol_package_app_error,
};
#[path = "protocol_packages/disposition.rs"]
mod disposition;
pub(in crate::adapters) use disposition::PreparedProtocolPackageDisposition;
#[path = "protocol_packages/error.rs"]
mod error;
pub use error::{ProtocolPackageStorageError, ProtocolPackageStorageErrorCode};
#[path = "protocol_packages/cache.rs"]
mod cache;
use cache::CachedCompiledPackage;
#[path = "protocol_packages/builtin.rs"]
mod builtin;
#[path = "protocol_packages/prepared.rs"]
mod prepared;
pub(super) use prepared::PreparedProtocolPackage;
#[path = "protocol_packages/portability.rs"]
mod portability;
pub(in crate::adapters) mod runtime_snapshot;
#[path = "protocol_packages/summary.rs"]
mod summary;
use summary::summary_from_header;
pub use summary::{
    ProtocolPackageInstallOutcome, ProtocolPackageSummary, ProtocolPackageValidationStatus,
};
#[cfg(test)]
pub use summary::{ProtocolPackageRecoveryFailure, ProtocolPackageRecoveryReport};
#[cfg(test)]
#[path = "protocol_packages/tests.rs"]
mod tests;
