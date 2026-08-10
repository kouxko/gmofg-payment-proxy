//! 可移植 Workspace 文档的唯一编解码边界。
//!
//! 内存仓储、SQLite 适配器、桌面 UI 和未来 CLI/TUI 都调用这里，避免不同入口对
//! 敏感字段、证书引用和文档大小采用不同规则。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::{
    AppError, AppResult, PortableCertificateMaterial, ProxyWorkspace,
    document_security::is_secret_field, validate_certificate_materials,
    validate_portable_certificate_references,
};

pub const WORKSPACE_DOCUMENT_FORMAT_VERSION: u16 = 2;
pub const MAX_WORKSPACE_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDocument {
    pub format_version: u16,
    pub workspace: ProxyWorkspace,
    pub certificate_materials: Vec<PortableCertificateMaterial>,
}

impl WorkspaceDocument {
    pub fn validate(&self) -> AppResult<()> {
        if self.format_version != WORKSPACE_DOCUMENT_FORMAT_VERSION {
            return Err(AppError::new(
                "WORKSPACE_DOCUMENT_VERSION_UNSUPPORTED",
                format!(
                    "Workspace 文档版本 {} 不受支持；当前仅支持版本 {}。",
                    self.format_version, WORKSPACE_DOCUMENT_FORMAT_VERSION
                ),
            ));
        }
        self.workspace.validate().map_err(AppError::from)?;
        validate_portable_certificate_references(&self.workspace)?;
        validate_certificate_materials(
            std::slice::from_ref(&self.workspace),
            &self.certificate_materials,
        )
    }
}

/// 解析并验证可移植 Workspace，但不持久化也不重映射领域 ID。
pub fn parse_workspace_document(document: &[u8]) -> AppResult<WorkspaceDocument> {
    if document.len() > MAX_WORKSPACE_DOCUMENT_BYTES {
        return Err(AppError::new(
            "IMPORT_FAILED",
            "Workspace 文档超过 64 MiB 安全上限。",
        ));
    }
    let value = serde_json::from_slice::<Value>(document)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("Workspace JSON 无效：{error}")))?;
    reject_workspace_fields_outside_certificate_materials(&value)?;
    let parsed = serde_json::from_value::<WorkspaceDocument>(value)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("Workspace 结构无效：{error}")))?;
    parsed.validate()?;
    Ok(parsed)
}

/// 序列化经过领域校验的 Workspace，并对输出再次执行敏感字段扫描。
pub fn serialize_workspace_document(document: &WorkspaceDocument) -> AppResult<Vec<u8>> {
    document.validate()?;
    let document = serde_json::to_vec_pretty(document).map_err(|error| {
        AppError::new("EXPORT_FAILED", format!("Workspace 序列化失败：{error}"))
    })?;
    let value = serde_json::from_slice::<Value>(&document).map_err(|error| {
        AppError::new("EXPORT_FAILED", format!("Workspace 导出自检失败：{error}"))
    })?;
    reject_workspace_fields_outside_certificate_materials(&value)
        .map_err(|_| AppError::new("EXPORT_FAILED", "Workspace 包含禁止导出的敏感字段。"))?;
    Ok(document)
}

fn reject_workspace_fields_outside_certificate_materials(value: &Value) -> AppResult<()> {
    let mut scanned = value.clone();
    if let Some(object) = scanned.as_object_mut() {
        object.insert("certificate_materials".into(), Value::Array(Vec::new()));
    }
    reject_sensitive_workspace_fields(&scanned, "$")
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
