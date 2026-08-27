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
    StoredProtocolPackage, StoredProtocolPackageFiles, StoredProtocolPackageValidation,
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
    pub(in crate::adapters) async fn observe_generation(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Uuid> {
        let selected = package.clone();
        self.executor
            .execute(move |store| {
                store
                    .load_protocol_package(&selected)
                    .map_err(ProtocolPackageStorageError::from)
                    .map_err(|error| protocol_package_app_error(&error))
            })
            .await?
            .map(|stored| stored.header.generation)
            .ok_or_else(|| {
                package_error(
                    package,
                    "PROTOCOL_PACKAGE_NOT_FOUND",
                    "指定的协议包精确版本尚未安装。",
                )
            })
    }

    /// 启动边界专用的无缓存恢复；不会读取或写入派生 AST 缓存。
    pub(in crate::adapters) async fn freeze_for_listener_start_async(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<RuntimeProtocolPackageSnapshot> {
        let selected = package.clone();
        let stored = self
            .executor
            .execute(move |store| {
                store
                    .load_protocol_package(&selected)
                    .map_err(ProtocolPackageStorageError::from)
                    .map_err(|error| protocol_package_app_error(&error))
            })
            .await?
            .ok_or_else(|| {
                package_error(
                    package,
                    "PROTOCOL_PACKAGE_NOT_FOUND",
                    "指定的协议包精确版本尚未安装。",
                )
            })?;
        let archive_limits = self.archive_limits.clone();
        let compiler = self.compiler;
        let runtime_limits = self.runtime_limits;
        let selected = package.clone();
        let compile_permit = Arc::clone(&self.runtime_compile_gate)
            .acquire_owned()
            .await
            .map_err(|_| {
                package_error(
                    package,
                    "PROTOCOL_PACKAGE_RUNTIME_PREPARE_FAILED",
                    "协议包运行快照准备执行器已关闭。",
                )
            })?;
        tokio::task::spawn_blocking(move || {
            let _compile_permit = compile_permit;
            freeze_loaded(&selected, stored, &archive_limits, compiler, runtime_limits)
        })
        .await
        .map_err(|_| {
            package_error(
                package,
                "PROTOCOL_PACKAGE_RUNTIME_PREPARE_FAILED",
                "协议包运行快照准备任务异常终止。",
            )
        })?
    }

    #[cfg(test)]
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
        freeze_loaded(
            package,
            stored,
            &self.archive_limits,
            self.compiler,
            self.runtime_limits,
        )
    }
}

fn freeze_loaded(
    package: &ProtocolPackageRef,
    stored: StoredProtocolPackage,
    archive_limits: &intercept_proxy_protocol_scripting::ProtocolArchiveLimits,
    package_compiler: intercept_proxy_protocol_scripting::ProtocolPackageCompiler,
    runtime_limits: ProtocolRuntimeLimits,
) -> AppResult<RuntimeProtocolPackageSnapshot> {
    if !stored.header.enabled {
        return Err(package_error(
            package,
            "PROTOCOL_PACKAGE_DISABLED",
            "Listener 引用的协议包版本已停用，请先启用后再启动。",
        ));
    }
    // 除持久化元数据损坏外，validation 只是上一次编译留下的历史结果。启动边界必须
    // 用当前 Host/compiler 对规范文件重新验证，不能让缓存的失败状态压过 fresh compile。
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
    let files = restore_protocol_package_files(raw_files, archive_limits)
        .map_err(|source| ProtocolPackageStorageError::Archive { source })
        .map_err(|error| protocol_package_app_error(&error))?;
    let compiled = package_compiler
        .compile(&files)
        .map_err(|source| ProtocolPackageStorageError::Compilation { source })
        .map_err(|error| protocol_package_app_error(&error))?;
    if compiled.package() != package
        || compiled.manifest().package().name() != stored.header.name
        || compiled.manifest().api() != stored.header.host_api
        || compiled.kind() != stored.header.kind
    {
        return Err(package_error(
            package,
            "STORED_IDENTITY_MISMATCH",
            "协议包持久化身份与重新编译结果不一致。",
        ));
    }
    Ok(RuntimeProtocolPackageSnapshot {
        compiled: Arc::new(compiled),
        runtime_limits,
        generation: stored.header.generation,
    })
}

fn package_error(package: &ProtocolPackageRef, code: &str, message: &str) -> AppError {
    AppError::new(code, message).entity(format!("{}@{}", package.id, package.version))
}
