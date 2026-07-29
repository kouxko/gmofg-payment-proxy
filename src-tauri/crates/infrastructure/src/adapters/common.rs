use gmofg_proxy_application::{AppError, AppResult};

use crate::{InfrastructureError, InfrastructureErrorCode};

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn app_error(error: InfrastructureError) -> AppError {
    let (code, message) = match error.code() {
        InfrastructureErrorCode::DatabaseMigrationFailed => {
            ("DATABASE_MIGRATION_FAILED", "数据库初始化失败。")
        }
        InfrastructureErrorCode::DatabaseWriteFailed => ("INTERNAL_ERROR", "数据库操作失败。"),
        InfrastructureErrorCode::RevisionConflict => {
            ("REVISION_CONFLICT", "数据已被其他操作更新，请重新加载。")
        }
        InfrastructureErrorCode::DpapiProtectFailed => {
            ("DPAPI_PROTECT_FAILED", "Windows 当前用户密钥保护失败。")
        }
        InfrastructureErrorCode::DpapiUnprotectFailed
        | InfrastructureErrorCode::DpapiUnsupported => (
            "DPAPI_UNPROTECT_FAILED",
            "Windows 当前用户密钥解密失败或当前平台不支持。",
        ),
        InfrastructureErrorCode::KeychainProtectFailed => (
            "KEYCHAIN_PROTECT_FAILED",
            "macOS Keychain 密钥保护失败，请确认登录钥匙串已解锁。",
        ),
        InfrastructureErrorCode::KeychainUnprotectFailed => (
            "KEYCHAIN_UNPROTECT_FAILED",
            "macOS Keychain 密钥解密失败，请确认登录钥匙串可访问。",
        ),
        InfrastructureErrorCode::CertificateInvalid => {
            ("CERTIFICATE_INVALID", "证书内容无效或不完整。")
        }
        InfrastructureErrorCode::Pkcs12PasswordInvalid => {
            ("PKCS12_PASSWORD_INVALID", "PKCS12 密码错误。")
        }
        InfrastructureErrorCode::ImportFailed => (
            "IMPORT_FAILED",
            "文件导入失败，请确认文件可读取且格式正确。",
        ),
        InfrastructureErrorCode::ExportFailed => (
            "EXPORT_FAILED",
            "文件导出失败，请确认目标目录可写且已确认覆盖。",
        ),
    };
    tracing::warn!(error = ?error, code, "infrastructure operation failed");
    AppError::new(code, message)
}

pub(crate) fn infra<T>(result: Result<T, InfrastructureError>) -> AppResult<T> {
    result.map_err(app_error)
}

pub(crate) fn json_error(context: &str, error: impl std::fmt::Display) -> AppError {
    AppError::new("INTERNAL_ERROR", format!("{context}：{error}"))
}
