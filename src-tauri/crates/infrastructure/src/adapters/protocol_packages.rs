//! 应用级协议包注册表适配器。
//!
//! 导入时先在数据库事务之外完成 ZIP 安全读取和 Rhai 编译，只有完整成功后才写入 SQLite。数据库只保存
//! 规范化文件；编译缓存是可丢弃的进程内派生物，重启后通过相同校验链恢复。

use std::{collections::HashMap, io::Cursor, sync::Arc};

use chrono::{DateTime, Utc};
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_protocol_scripting::{
    CompiledProtocolPackage, ProtocolArchiveError, ProtocolArchiveLimits,
    ProtocolPackageCompilationError, ProtocolPackageCompiler, ProtocolRuntimeLimits,
    read_protocol_package_zip, restore_protocol_package_files,
};
use parking_lot::Mutex;
use thiserror::Error;
use uuid::Uuid;

use crate::{InfrastructureError, SqliteStore};

use super::super::sqlite::protocol_packages::{
    StoredProtocolPackageFiles, StoredProtocolPackageHeader, StoredProtocolPackageInstallOutcome,
    StoredProtocolPackageValidation,
};

/// 无源码的持久化校验状态。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolPackageValidationStatus {
    /// 最近一次导入或缓存恢复完整通过。
    Valid,
    /// 数据库文件集合无法再通过路径、声明或 Rhai 编译校验。
    Invalid {
        /// 稳定机器码；不包含脚本内容、原始路径或第三方错误文本。
        code: String,
    },
}

/// 协议包列表和后续 Application 用例使用的无源码记录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolPackageSummary {
    /// 不可变的应用级 ID 与精确 `SemVer`。
    pub package: ProtocolPackageRef,
    /// Manifest 中受长度和控制字符门禁保护的展示名称。
    pub name: String,
    /// 导入时已经确认受当前 Host 支持的 API 主版本。
    pub host_api: u32,
    /// 应用级启用位；新安装记录固定为 `false`。
    pub enabled: bool,
    /// 最近一次完整编译或缓存恢复结果。
    pub validation: ProtocolPackageValidationStatus,
    /// 首次安装时间；幂等重入不会改写。
    pub installed_at: DateTime<Utc>,
}

/// 幂等导入结果；相同身份与完全相同文件集合会复用现有记录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolPackageInstallOutcome {
    Installed(ProtocolPackageSummary),
    Reused(ProtocolPackageSummary),
}

/// 启动时单个包的缓存恢复失败。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolPackageRecoveryFailure {
    /// 无法恢复的精确版本。
    pub package: ProtocolPackageRef,
    /// 路径、声明或脚本阶段产生的稳定脱敏机器码。
    pub code: String,
}

/// 启动缓存恢复报告；一个坏包不会阻止其他独立版本恢复。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProtocolPackageRecoveryReport {
    /// 已重新编译并进入进程缓存的版本。
    pub loaded: Vec<ProtocolPackageRef>,
    /// 已持久化标记为 Invalid 且没有进入缓存的版本。
    pub failed: Vec<ProtocolPackageRecoveryFailure>,
}

/// 协议包存储边界的稳定错误分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolPackageStorageErrorCode {
    ArchiveInvalid,
    CompilationFailed,
    IdentityConflict,
    NotFound,
    StoredPackageInvalid,
    PersistenceFailed,
}

/// 导入、恢复和持久化失败；公开消息不包含 ZIP/Rhai 源码。
#[derive(Debug, Error)]
pub enum ProtocolPackageStorageError {
    #[error("协议包 ZIP 校验失败")]
    Archive {
        #[source]
        source: ProtocolArchiveError,
    },
    #[error("协议包声明或脚本编译失败")]
    Compilation {
        #[source]
        source: ProtocolPackageCompilationError,
    },
    #[error("相同协议包 ID 与版本已经安装，但内容不同")]
    IdentityConflict { package: ProtocolPackageRef },
    #[error("协议包未安装")]
    NotFound { package: ProtocolPackageRef },
    #[error("已存储协议包无法通过重新校验（{code}）")]
    StoredPackageInvalid {
        package: ProtocolPackageRef,
        code: String,
    },
    #[error(transparent)]
    Infrastructure(#[from] InfrastructureError),
}

impl ProtocolPackageStorageError {
    #[must_use]
    pub const fn code(&self) -> ProtocolPackageStorageErrorCode {
        match self {
            Self::Archive { .. } => ProtocolPackageStorageErrorCode::ArchiveInvalid,
            Self::Compilation { .. } => ProtocolPackageStorageErrorCode::CompilationFailed,
            Self::IdentityConflict { .. } => ProtocolPackageStorageErrorCode::IdentityConflict,
            Self::NotFound { .. } => ProtocolPackageStorageErrorCode::NotFound,
            Self::StoredPackageInvalid { .. } => {
                ProtocolPackageStorageErrorCode::StoredPackageInvalid
            }
            Self::Infrastructure(_) => ProtocolPackageStorageErrorCode::PersistenceFailed,
        }
    }

    /// 返回导入/恢复阶段更精确的稳定机器码，供后续 Dialog 映射；数据库错误由应用公共错误映射处理。
    #[must_use]
    pub fn detail_code(&self) -> Option<&str> {
        match self {
            Self::Archive { source } => Some(source.code().as_str()),
            Self::Compilation { source } => Some(compilation_code(source)),
            Self::StoredPackageInvalid { code, .. } => Some(code),
            Self::IdentityConflict { .. } => Some("PROTOCOL_PACKAGE_IDENTITY_CONFLICT"),
            Self::NotFound { .. } => Some("PROTOCOL_PACKAGE_NOT_FOUND"),
            Self::Infrastructure(_) => None,
        }
    }
}

/// `SQLite` 持久化与可重建 Rhai 编译缓存的组合适配器。
#[derive(Debug)]
pub struct ProtocolPackageRepositoryAdapter {
    store: Arc<SqliteStore>,
    archive_limits: ProtocolArchiveLimits,
    compiler: ProtocolPackageCompiler,
    cache: Mutex<HashMap<ProtocolPackageRef, CachedCompiledPackage>>,
}

#[derive(Debug)]
struct CachedCompiledPackage {
    generation: Uuid,
    compiled: Arc<CompiledProtocolPackage>,
}

impl ProtocolPackageRepositoryAdapter {
    /// 使用显式资源限制创建注册表；测试和未来配置入口可收紧默认值。
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        archive_limits: ProtocolArchiveLimits,
        runtime_limits: ProtocolRuntimeLimits,
    ) -> Self {
        Self {
            store,
            archive_limits,
            compiler: ProtocolPackageCompiler::new(runtime_limits),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 创建使用 Host 默认安全门禁的注册表。
    #[must_use]
    pub fn with_default_limits(store: Arc<SqliteStore>) -> Self {
        Self::new(
            store,
            ProtocolArchiveLimits::default(),
            ProtocolRuntimeLimits::default(),
        )
    }

    /// 完整校验 ZIP 后原子安装。新包默认停用；相同内容重入不改变已有启用状态和安装时间。
    pub fn install_zip(
        &self,
        zip_bytes: &[u8],
    ) -> Result<ProtocolPackageInstallOutcome, ProtocolPackageStorageError> {
        let files = read_protocol_package_zip(Cursor::new(zip_bytes), &self.archive_limits)
            .map_err(|source| ProtocolPackageStorageError::Archive { source })?;
        let compiled = self
            .compiler
            .compile(&files)
            .map_err(|source| ProtocolPackageStorageError::Compilation { source })?;
        let manifest = compiled.manifest();
        let header = StoredProtocolPackageHeader {
            package: compiled.package().clone(),
            name: manifest.package().name().to_owned(),
            host_api: manifest.api(),
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

    /// 按 ID、版本排序列出无源码记录。该操作只反映最近一次恢复状态，不隐式执行脚本编译。
    pub fn list(&self) -> Result<Vec<ProtocolPackageSummary>, ProtocolPackageStorageError> {
        Ok(self
            .store
            .list_protocol_package_headers()?
            .into_iter()
            .map(summary_from_header)
            .collect())
    }

    /// 读取一个无源码精确版本记录。
    pub fn summary(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Option<ProtocolPackageSummary>, ProtocolPackageStorageError> {
        Ok(self
            .store
            .load_protocol_package_header(package)?
            .map(summary_from_header))
    }

    /// 获取可执行编译产物。缓存缺失时重新验证数据库路径、文件限制、Manifest、Schema 和 Rhai。
    pub fn compiled(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Arc<CompiledProtocolPackage>, ProtocolPackageStorageError> {
        let mut cache = self.cache.lock();
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
            // 另一 SqliteStore/适配器也可能删除同一记录，所以缓存命中仍以轻量 header 查询确认
            // 持久化身份存在。查询先发生时，本次读取可线性化在并发删除之前。
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
            // 同一身份已被其他适配器删除并重装。代际不匹配时必须放弃旧 AST，走完整恢复链。
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

    /// 只持久化启用位；能否启停以及引用约束由 T14 Application 用例决定。
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

    fn require_validation_update(
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

fn summary_from_header(header: StoredProtocolPackageHeader) -> ProtocolPackageSummary {
    let validation = match header.validation {
        StoredProtocolPackageValidation::Valid => ProtocolPackageValidationStatus::Valid,
        StoredProtocolPackageValidation::Invalid(code) => {
            ProtocolPackageValidationStatus::Invalid { code }
        }
    };
    ProtocolPackageSummary {
        package: header.package,
        name: header.name,
        host_api: header.host_api,
        enabled: header.enabled,
        validation,
        installed_at: header.installed_at,
    }
}

fn compilation_code(error: &ProtocolPackageCompilationError) -> &str {
    if let Some(error) = error.declaration_error() {
        error.code().as_str()
    } else if let Some(error) = error.script_error() {
        error.code().as_str()
    } else {
        // 枚举当前只有 Declaration/Script；保留稳定兜底避免未来新增变体时泄漏 Display 文本。
        "COMPILATION_FAILED"
    }
}

#[cfg(test)]
#[path = "protocol_packages/tests.rs"]
mod tests;
