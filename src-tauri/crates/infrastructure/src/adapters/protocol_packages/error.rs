//! 协议包导入、恢复和持久化边界的稳定错误。

use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_protocol_scripting::{ProtocolArchiveError, ProtocolPackageCompilationError};
use thiserror::Error;

use crate::InfrastructureError;

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
    #[error("可移植协议包文件载荷无效")]
    PortableInvalid,
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
            Self::Archive { .. } | Self::PortableInvalid => {
                ProtocolPackageStorageErrorCode::ArchiveInvalid
            }
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
            Self::PortableInvalid => Some("PORTABLE_PROTOCOL_PACKAGE_INVALID"),
            Self::Compilation { source } => Some(compilation_code(source)),
            Self::StoredPackageInvalid { code, .. } => Some(code),
            Self::IdentityConflict { .. } => Some("PROTOCOL_PACKAGE_IDENTITY_CONFLICT"),
            Self::NotFound { .. } => Some("PROTOCOL_PACKAGE_NOT_FOUND"),
            Self::Infrastructure(_) => None,
        }
    }
}

pub(super) fn compilation_code(error: &ProtocolPackageCompilationError) -> &str {
    error
        .declaration_error()
        .map(|error| error.code().as_str())
        .or_else(|| error.script_error().map(|error| error.code().as_str()))
        .unwrap_or("COMPILATION_FAILED")
}
