//! 基础设施错误到应用错误的公共映射。
//!
//! 集中映射可保持前端错误码稳定，并保证底层错误链
//! 不会把敏感内容直接暴露给 `WebView`。

use intercept_proxy_application::{
    AppError, AppResult, ProxyWorkspace, WORKSPACE_PERSISTENCE_VERSION,
};

use crate::{InfrastructureError, InfrastructureErrorCode, WorkspaceRecord};

/// 解码一条 Workspace 记录并同时核对 `SQLite` 索引与 JSON 聚合版本。
///
/// 数据面查找 Listener 时必须逐条验证；不能用 `filter_map(...ok())` 跳过坏记录，
/// 否则数据库损坏会伪装成“没有 Codec/规则/断言”，造成静默直通。
pub(crate) fn decode_workspace_record(record: WorkspaceRecord) -> Result<ProxyWorkspace, String> {
    let indexed_id = record.id;
    let indexed_revision = record.revision;
    let mut value = record.value;
    let version = take_workspace_persistence_version(&mut value)?;
    if version != u64::from(WORKSPACE_PERSISTENCE_VERSION) {
        return Err(format!(
            "Workspace {indexed_id} 持久化版本 {version} 不受支持；当前仅支持版本 {WORKSPACE_PERSISTENCE_VERSION}"
        ));
    }
    let workspace = serde_json::from_value::<ProxyWorkspace>(value)
        .map_err(|error| format!("Workspace {indexed_id} v{version} 结构无效：{error}"))?;
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

pub(crate) fn encode_workspace_record(
    workspace: &ProxyWorkspace,
) -> Result<serde_json::Value, String> {
    let mut value = serde_json::to_value(workspace)
        .map_err(|error| format!("Workspace 序列化失败：{error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Workspace 序列化结果不是 JSON object".to_owned())?;
    object.insert(
        "_persistence_version".into(),
        serde_json::json!(WORKSPACE_PERSISTENCE_VERSION),
    );
    Ok(value)
}

fn take_workspace_persistence_version(value: &mut serde_json::Value) -> Result<u64, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "Workspace 持久化值不是 JSON object".to_owned())?;
    object
        .remove("_persistence_version")
        .ok_or_else(|| "Workspace _persistence_version 缺失".to_owned())?
        .as_u64()
        .ok_or_else(|| "Workspace _persistence_version 必须是整数".to_owned())
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn app_error(error: InfrastructureError) -> AppError {
    let (code, message) = match error.code() {
        InfrastructureErrorCode::DatabaseSchemaFailed => {
            ("DATABASE_SCHEMA_INVALID", "数据库结构初始化或校验失败。")
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

impl From<InfrastructureError> for AppError {
    fn from(error: InfrastructureError) -> Self {
        app_error(error)
    }
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
        for (id, revision) in [
            (uuid::Uuid::new_v4(), workspace.revision.get()),
            (workspace.id.as_uuid(), workspace.revision.get() + 1),
        ] {
            let record = WorkspaceRecord {
                id,
                revision,
                value: encode_workspace_record(&workspace).expect("workspace JSON"),
                updated_at: Utc::now(),
            };
            let error = decode_workspace_record(record).expect_err("mismatched index must fail");
            assert!(error.contains("索引 ID/revision"), "{error}");
        }
    }

    #[test]
    fn workspace_record_decoder_rejects_unknown_fields_and_versions() {
        let workspace = ProxyWorkspace::default();
        for value in [
            {
                let mut value = encode_workspace_record(&workspace).expect("workspace JSON");
                value["proxy_password"] = serde_json::json!("must-not-leak");
                value
            },
            {
                let mut value = encode_workspace_record(&workspace).expect("workspace JSON");
                value["_persistence_version"] = serde_json::json!(99);
                value
            },
        ] {
            let record = WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value,
                updated_at: Utc::now(),
            };
            let error = decode_workspace_record(record).expect_err("corrupt workspace must fail");
            assert!(!error.contains("must-not-leak"));
        }
    }

    #[test]
    fn workspace_record_decoder_rejects_invalid_discriminator_and_damaged_record() {
        let workspace = ProxyWorkspace::default();
        for value in [
            {
                let mut value = encode_workspace_record(&workspace).expect("workspace JSON");
                value["_persistence_version"] = serde_json::json!("3");
                value
            },
            {
                let mut value = encode_workspace_record(&workspace).expect("workspace JSON");
                value
                    .as_object_mut()
                    .unwrap()
                    .remove("_persistence_version");
                value
            },
        ] {
            let record = WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value,
                updated_at: Utc::now(),
            };
            decode_workspace_record(record).expect_err("invalid version shape must fail closed");
        }
    }
}
