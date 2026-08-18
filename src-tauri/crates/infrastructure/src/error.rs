//! 基础设施错误的稳定分类与脱敏展示。
//!
//! 底层库错误在这里被归并成应用可识别的错误码；消息可以说明失败阶段和路径，但禁止
//! 携带证书私钥、口令、HTTP 载荷等敏感字节。

use std::path::PathBuf;

use thiserror::Error;

/// Stable infrastructure error categories mapped to application error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfrastructureErrorCode {
    DatabaseSchemaFailed,
    DatabaseWriteFailed,
    RevisionConflict,
    DpapiProtectFailed,
    DpapiUnprotectFailed,
    DpapiUnsupported,
    KeychainProtectFailed,
    KeychainUnprotectFailed,
    CertificateInvalid,
    Pkcs12PasswordInvalid,
    ImportFailed,
    ImportTooLarge,
    PersistenceCorrupt,
    ExportFailed,
}

/// Infrastructure failures deliberately omit secret data and payload bytes.
#[derive(Debug, Error)]
pub enum InfrastructureError {
    #[error("数据库结构初始化或校验失败")]
    DatabaseSchema {
        #[source]
        source: rusqlite::Error,
    },
    #[error("数据库结构版本无效：当前版本 {current}，实际标记 {found:?}")]
    DatabaseSchemaInvalid {
        current: i64,
        found: Vec<(i64, i64)>,
    },
    #[error("数据库操作失败")]
    Database {
        #[source]
        source: rusqlite::Error,
    },
    #[error("数据已被其他操作更新")]
    RevisionConflict,
    #[error("当前平台不支持 Windows DPAPI")]
    DpapiUnsupported,
    #[error("DPAPI 保护敏感数据失败")]
    DpapiProtect,
    #[error("DPAPI 解密敏感数据失败")]
    DpapiUnprotect,
    #[error("macOS Keychain 保护敏感数据失败")]
    KeychainProtect,
    #[error("macOS Keychain 解密敏感数据失败")]
    KeychainUnprotect,
    #[error("证书无效：{message}")]
    CertificateInvalid { message: String },
    #[error("PKCS12 密码错误")]
    Pkcs12PasswordInvalid,
    #[error("导入文件超过大小限制（最大 {max_bytes} 字节）：{path}")]
    ImportTooLarge {
        path: PathBuf,
        max_bytes: u64,
        actual_bytes: Option<u64>,
    },
    #[error("持久化数据损坏（{entity}）：{message}")]
    PersistenceCorrupt {
        entity: &'static str,
        message: String,
    },
    #[error("文件导入失败：{path}")]
    Import {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("文件导出失败：{path}")]
    Export {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("目标文件已替换，但父目录持久化状态无法确认：{path}")]
    ExportParentSync {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("目标文件已存在：{path}")]
    ExportTargetExists { path: PathBuf },
}

impl InfrastructureError {
    #[must_use]
    pub const fn code(&self) -> InfrastructureErrorCode {
        match self {
            Self::DatabaseSchema { .. } | Self::DatabaseSchemaInvalid { .. } => {
                InfrastructureErrorCode::DatabaseSchemaFailed
            }
            Self::Database { .. } => InfrastructureErrorCode::DatabaseWriteFailed,
            Self::RevisionConflict => InfrastructureErrorCode::RevisionConflict,
            Self::DpapiUnsupported => InfrastructureErrorCode::DpapiUnsupported,
            Self::DpapiProtect => InfrastructureErrorCode::DpapiProtectFailed,
            Self::DpapiUnprotect => InfrastructureErrorCode::DpapiUnprotectFailed,
            Self::KeychainProtect => InfrastructureErrorCode::KeychainProtectFailed,
            Self::KeychainUnprotect => InfrastructureErrorCode::KeychainUnprotectFailed,
            Self::CertificateInvalid { .. } => InfrastructureErrorCode::CertificateInvalid,
            Self::Pkcs12PasswordInvalid => InfrastructureErrorCode::Pkcs12PasswordInvalid,
            Self::ImportTooLarge { .. } => InfrastructureErrorCode::ImportTooLarge,
            Self::PersistenceCorrupt { .. } => InfrastructureErrorCode::PersistenceCorrupt,
            Self::Import { .. } => InfrastructureErrorCode::ImportFailed,
            Self::Export { .. }
            | Self::ExportParentSync { .. }
            | Self::ExportTargetExists { .. } => InfrastructureErrorCode::ExportFailed,
        }
    }
}
