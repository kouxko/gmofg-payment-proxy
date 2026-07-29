use std::path::PathBuf;

use thiserror::Error;

/// Stable infrastructure error categories mapped to application error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfrastructureErrorCode {
    DatabaseMigrationFailed,
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
    ExportFailed,
}

/// Infrastructure failures deliberately omit secret data and payload bytes.
#[derive(Debug, Error)]
pub enum InfrastructureError {
    #[error("数据库迁移失败")]
    DatabaseMigration {
        #[source]
        source: rusqlite::Error,
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
    #[error("目标文件已存在：{path}")]
    ExportTargetExists { path: PathBuf },
}

impl InfrastructureError {
    #[must_use]
    pub const fn code(&self) -> InfrastructureErrorCode {
        match self {
            Self::DatabaseMigration { .. } => InfrastructureErrorCode::DatabaseMigrationFailed,
            Self::Database { .. } => InfrastructureErrorCode::DatabaseWriteFailed,
            Self::RevisionConflict => InfrastructureErrorCode::RevisionConflict,
            Self::DpapiUnsupported => InfrastructureErrorCode::DpapiUnsupported,
            Self::DpapiProtect => InfrastructureErrorCode::DpapiProtectFailed,
            Self::DpapiUnprotect => InfrastructureErrorCode::DpapiUnprotectFailed,
            Self::KeychainProtect => InfrastructureErrorCode::KeychainProtectFailed,
            Self::KeychainUnprotect => InfrastructureErrorCode::KeychainUnprotectFailed,
            Self::CertificateInvalid { .. } => InfrastructureErrorCode::CertificateInvalid,
            Self::Pkcs12PasswordInvalid => InfrastructureErrorCode::Pkcs12PasswordInvalid,
            Self::Import { .. } => InfrastructureErrorCode::ImportFailed,
            Self::Export { .. } | Self::ExportTargetExists { .. } => {
                InfrastructureErrorCode::ExportFailed
            }
        }
    }
}
