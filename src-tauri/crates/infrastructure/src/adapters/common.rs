//! 基础设施错误到应用错误的公共映射。
//!
//! 集中映射可保持前端错误码稳定，并保证底层错误链不会把敏感内容直接暴露给 `WebView`。

use intercept_proxy_application::{AppError, AppResult, ProxyWorkspace};

use crate::{InfrastructureError, InfrastructureErrorCode, WorkspaceRecord};

/// 解码一条 Workspace 记录并同时核对 `SQLite` 索引与 JSON 聚合版本。
///
/// 数据面查找 Listener 时必须逐条验证；不能用 `filter_map(...ok())` 跳过坏记录，
/// 否则数据库损坏会伪装成“没有 Codec/规则/断言”，造成静默直通。
pub(crate) fn decode_workspace_record(record: WorkspaceRecord) -> Result<ProxyWorkspace, String> {
    let indexed_id = record.id;
    let indexed_revision = record.revision;
    let workspace = serde_json::from_value::<ProxyWorkspace>(record.value)
        .map_err(|error| format!("Workspace {indexed_id} 结构无效：{error}"))?;
    if workspace.id.as_uuid() != indexed_id || workspace.revision.get() != indexed_revision {
        return Err(format!(
            "Workspace {indexed_id} 的索引 ID/revision 与 JSON 内容不一致"
        ));
    }
    workspace
        .validate()
        .map_err(|error| format!("Workspace {indexed_id} 领域校验失败：{error}"))?;
    Ok(workspace)
}

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
        InfrastructureErrorCode::ImportTooLarge => (
            "IMPORT_TOO_LARGE",
            "导入文件超过允许的大小限制，请选择更小的文件。",
        ),
        InfrastructureErrorCode::PersistenceCorrupt => (
            "PERSISTENCE_CORRUPT",
            "本地持久化数据已损坏，请修复或重置相关数据。",
        ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn import_size_and_persistence_corruption_have_stable_app_codes() {
        let oversized = app_error(InfrastructureError::ImportTooLarge {
            path: "oversized.bin".into(),
            max_bytes: 16,
            actual_bytes: Some(17),
        });
        assert_eq!(oversized.view_model.code, "IMPORT_TOO_LARGE");

        let corrupt = app_error(InfrastructureError::PersistenceCorrupt {
            entity: "rule",
            message: "invalid JSON".into(),
        });
        assert_eq!(corrupt.view_model.code, "PERSISTENCE_CORRUPT");
        assert_ne!(corrupt.view_model.code, "CERTIFICATE_INVALID");
    }

    #[test]
    fn workspace_record_decoder_rejects_index_json_mismatch() {
        let workspace = ProxyWorkspace::default();
        let record = WorkspaceRecord {
            id: uuid::Uuid::new_v4(),
            revision: workspace.revision.get(),
            value: serde_json::to_value(workspace).expect("workspace JSON"),
            updated_at: Utc::now(),
        };

        let error = decode_workspace_record(record).expect_err("mismatched index must fail");
        assert!(error.contains("索引 ID/revision"), "{error}");
    }
}
