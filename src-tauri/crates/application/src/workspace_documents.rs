//! 可移植 Workspace 文档的唯一编解码边界。
//!
//! 内存仓储、SQLite 适配器、桌面 UI 和未来 CLI/TUI 都调用这里，避免不同入口对
//! 敏感字段、证书引用和文档大小采用不同规则。

use serde_json::Value;

use crate::{
    AppError, AppResult, ProxyWorkspace, document_security::is_secret_field,
    validate_portable_certificate_references,
};

pub const MAX_WORKSPACE_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

/// 解析并验证可移植 Workspace，但不持久化也不重映射领域 ID。
pub fn parse_workspace_document(document: &[u8]) -> AppResult<ProxyWorkspace> {
    if document.len() > MAX_WORKSPACE_DOCUMENT_BYTES {
        return Err(AppError::new(
            "IMPORT_FAILED",
            "Workspace 文档超过 8 MiB 安全上限。",
        ));
    }
    let value = serde_json::from_slice::<Value>(document)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("Workspace JSON 无效：{error}")))?;
    reject_sensitive_workspace_fields(&value, "$")?;
    let workspace = serde_json::from_value::<ProxyWorkspace>(value)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("Workspace 结构无效：{error}")))?;
    workspace.validate().map_err(AppError::from)?;
    validate_portable_certificate_references(&workspace)?;
    Ok(workspace)
}

/// 序列化经过领域校验的 Workspace，并对输出再次执行敏感字段扫描。
pub fn serialize_workspace_document(workspace: &ProxyWorkspace) -> AppResult<Vec<u8>> {
    workspace.validate().map_err(AppError::from)?;
    validate_portable_certificate_references(workspace).map_err(|_| {
        AppError::new(
            "EXPORT_FAILED",
            "Workspace 包含不能导出的外部证书引用，请在代理入口中重新导入证书。",
        )
    })?;
    let document = serde_json::to_vec_pretty(workspace).map_err(|error| {
        AppError::new("EXPORT_FAILED", format!("Workspace 序列化失败：{error}"))
    })?;
    let value = serde_json::from_slice::<Value>(&document).map_err(|error| {
        AppError::new("EXPORT_FAILED", format!("Workspace 导出自检失败：{error}"))
    })?;
    reject_sensitive_workspace_fields(&value, "$")
        .map_err(|_| AppError::new("EXPORT_FAILED", "Workspace 包含禁止导出的敏感字段。"))?;
    Ok(document)
}

fn reject_sensitive_workspace_fields(value: &Value, path: &str) -> AppResult<()> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if is_secret_field(key) {
                    return Err(AppError::new(
                        "IMPORT_FAILED",
                        format!("Workspace 文档包含禁止的敏感字段：{path}.{key}"),
                    ));
                }
                reject_sensitive_workspace_fields(value, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                reject_sensitive_workspace_fields(value, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn nested_camel_case_secret_is_rejected_before_serde_can_ignore_it() {
        let mut value = serde_json::to_value(ProxyWorkspace::default()).expect("workspace value");
        value["extension"] = json!({"credentials": {"privateKey": "forbidden"}});

        let error = parse_workspace_document(
            &serde_json::to_vec(&value).expect("workspace document bytes"),
        )
        .expect_err("unknown nested secret must be rejected");

        assert_eq!(error.view_model.code, "IMPORT_FAILED");
        assert!(error.view_model.message.contains("privateKey"));
    }
}
