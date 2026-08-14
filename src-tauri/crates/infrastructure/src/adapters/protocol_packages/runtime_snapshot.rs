//! Listener 启动时使用的无缓存协议包快照。
//!
//! 运行中的 Listener 不能引用可被注册表缓存替换的 AST。这里从一次 `SQLite` 读取取得
//! header 与规范文件，确认启用状态后重新执行路径、Manifest、Schema 和 Rhai 编译，最后
//! 返回只读 `Arc`。后续停用、删除或重新安装同一身份都不会改变已经冻结的对象。

use std::sync::Arc;

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_protocol_scripting::{
    CompiledProtocolPackage, ProtocolRuntimeLimits, restore_protocol_package_files,
};
use uuid::Uuid;

use crate::sqlite::protocol_packages::{
    StoredProtocolPackageFiles, StoredProtocolPackageValidation,
};

use super::{
    ProtocolPackageRepositoryAdapter, ProtocolPackageStorageError, protocol_package_app_error,
};

/// 从同一持久化记录冻结的可执行包与安全资源限制。
#[derive(Clone)]
pub(in crate::adapters) struct RuntimeProtocolPackageSnapshot {
    compiled: Arc<CompiledProtocolPackage>,
    runtime_limits: ProtocolRuntimeLimits,
    generation: Uuid,
}

impl RuntimeProtocolPackageSnapshot {
    pub(in crate::adapters) fn compiled(&self) -> &Arc<CompiledProtocolPackage> {
        &self.compiled
    }

    pub(in crate::adapters) const fn runtime_limits(&self) -> ProtocolRuntimeLimits {
        self.runtime_limits
    }

    #[cfg(test)]
    pub(in crate::adapters) const fn generation(&self) -> Uuid {
        self.generation
    }
}

impl std::fmt::Debug for RuntimeProtocolPackageSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeProtocolPackageSnapshot")
            .field("package", self.compiled.package())
            .field("generation", &self.generation)
            .field("runtime_limits", &self.runtime_limits)
            .finish_non_exhaustive()
    }
}

impl ProtocolPackageRepositoryAdapter {
    /// 启动边界专用的无缓存恢复；不会读取或写入派生 AST 缓存。
    pub(in crate::adapters) fn freeze_for_listener_start(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<RuntimeProtocolPackageSnapshot> {
        let stored = self
            .store
            .load_protocol_package(package)
            .map_err(ProtocolPackageStorageError::from)
            .map_err(|error| protocol_package_app_error(&error))?
            .ok_or_else(|| {
                package_error(
                    package,
                    "PROTOCOL_PACKAGE_NOT_FOUND",
                    "指定的协议包精确版本尚未安装。",
                )
            })?;
        if !stored.header.enabled {
            return Err(package_error(
                package,
                "PROTOCOL_PACKAGE_DISABLED",
                "Listener 引用的协议包版本已停用，请先启用后再启动。",
            ));
        }
        // 除持久化元数据损坏外，validation 只是上一次编译留下的历史结果。启动边界必须
        // 用当前 Host/compiler 对规范文件重新验证，不能让旧版本的失败永久压过 fresh compile。
        if matches!(
            stored.header.validation,
            StoredProtocolPackageValidation::Invalid(ref code) if code == "PERSISTENCE_CORRUPT"
        ) {
            return Err(package_error(
                package,
                "PERSISTENCE_CORRUPT",
                "协议包持久化元数据损坏，不能安全恢复运行快照。",
            ));
        }
        let raw_files = match stored.files {
            StoredProtocolPackageFiles::Valid(files) => files,
            StoredProtocolPackageFiles::Rejected(code) => {
                return Err(package_error(
                    package,
                    code,
                    "协议包持久化文件超过安全读取限制。",
                ));
            }
        };
        let files = restore_protocol_package_files(raw_files, &self.archive_limits)
            .map_err(|source| ProtocolPackageStorageError::Archive { source })
            .map_err(|error| protocol_package_app_error(&error))?;
        let compiled = self
            .compiler
            .compile(&files)
            .map_err(|source| ProtocolPackageStorageError::Compilation { source })
            .map_err(|error| protocol_package_app_error(&error))?;
        if compiled.package() != package
            || compiled.manifest().package().name() != stored.header.name
            || compiled.manifest().api() != stored.header.host_api
        {
            return Err(package_error(
                package,
                "STORED_IDENTITY_MISMATCH",
                "协议包持久化身份与重新编译结果不一致。",
            ));
        }
        Ok(RuntimeProtocolPackageSnapshot {
            compiled: Arc::new(compiled),
            runtime_limits: self.runtime_limits,
            generation: stored.header.generation,
        })
    }
}

fn package_error(package: &ProtocolPackageRef, code: &str, message: &str) -> AppError {
    AppError::new(code, message).entity(format!("{}@{}", package.id, package.version))
}
