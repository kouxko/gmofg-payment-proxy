//! 整个应用的可移植配置文档。
//!
//! `.intercept-config` 与单个 `.intercept-workspace` 文档分工明确：前者用于完整备份与
//! 恢复，后者用于分享一个 Workspace。两者都只允许非敏感配置和稳定安全引用。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::{
    AppError, AppResult, ChannelSettingsDraft, ProxyWorkspace, SettingsDraft, WorkspaceId,
};

pub const APPLICATION_CONFIGURATION_FORMAT_VERSION: u16 = 1;
pub const MAX_APPLICATION_CONFIGURATION_BYTES: usize = 32 * 1024 * 1024;

/// 不含乐观锁 revision 的全局设置值。
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
pub struct PortableSettings {
    pub bind_address: String,
    pub channels: Vec<ChannelSettingsDraft>,
    pub connect_timeout_seconds: u64,
    pub write_timeout_seconds: u64,
    pub read_timeout_seconds: u64,
    pub rewrite_host: bool,
    pub max_body_bytes: u64,
    pub max_sessions: usize,
    pub max_memory_bytes: u64,
    pub leaf_sans: Vec<String>,
}

impl From<&SettingsDraft> for PortableSettings {
    fn from(value: &SettingsDraft) -> Self {
        Self {
            bind_address: value.bind_address.clone(),
            channels: value.channels.clone(),
            connect_timeout_seconds: value.connect_timeout_seconds,
            write_timeout_seconds: value.write_timeout_seconds,
            read_timeout_seconds: value.read_timeout_seconds,
            rewrite_host: value.rewrite_host,
            max_body_bytes: value.max_body_bytes,
            max_sessions: value.max_sessions,
            max_memory_bytes: value.max_memory_bytes,
            leaf_sans: value.leaf_sans.clone(),
        }
    }
}

impl PortableSettings {
    #[must_use]
    pub fn to_draft(&self, expected_revision: Option<u64>) -> SettingsDraft {
        SettingsDraft {
            expected_revision,
            bind_address: self.bind_address.clone(),
            channels: self.channels.clone(),
            connect_timeout_seconds: self.connect_timeout_seconds,
            write_timeout_seconds: self.write_timeout_seconds,
            read_timeout_seconds: self.read_timeout_seconds,
            rewrite_host: self.rewrite_host,
            max_body_bytes: self.max_body_bytes,
            max_sessions: self.max_sessions,
            max_memory_bytes: self.max_memory_bytes,
            leaf_sans: self.leaf_sans.clone(),
        }
    }
}

/// 应用完整配置的唯一文档结构。
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ApplicationConfigurationDocument {
    pub format_version: u16,
    pub selected_workspace_id: WorkspaceId,
    pub workspaces: Vec<ProxyWorkspace>,
    pub settings: PortableSettings,
}

impl ApplicationConfigurationDocument {
    pub fn validate(&self) -> AppResult<()> {
        if self.format_version != APPLICATION_CONFIGURATION_FORMAT_VERSION {
            return Err(AppError::new(
                "APPLICATION_CONFIGURATION_VERSION_UNSUPPORTED",
                format!(
                    "配置文档版本 {} 不受支持；当前仅支持版本 {}。",
                    self.format_version, APPLICATION_CONFIGURATION_FORMAT_VERSION
                ),
            ));
        }
        if self.workspaces.is_empty() {
            return Err(AppError::new(
                "APPLICATION_CONFIGURATION_INVALID",
                "完整配置至少必须包含一个 Workspace。",
            ));
        }
        let mut ids = BTreeSet::new();
        for workspace in &self.workspaces {
            if !ids.insert(workspace.id) {
                return Err(AppError::new(
                    "APPLICATION_CONFIGURATION_INVALID",
                    "完整配置中的 Workspace ID 不能重复。",
                )
                .entity(workspace.id.to_string()));
            }
            workspace.validate().map_err(AppError::from)?;
        }
        if !ids.contains(&self.selected_workspace_id) {
            return Err(AppError::new(
                "APPLICATION_CONFIGURATION_INVALID",
                "当前选中的 Workspace 不存在于文档中。",
            )
            .entity(self.selected_workspace_id.to_string()));
        }
        Ok(())
    }
}

pub fn parse_application_configuration(
    document: &[u8],
) -> AppResult<ApplicationConfigurationDocument> {
    if document.len() > MAX_APPLICATION_CONFIGURATION_BYTES {
        return Err(AppError::new(
            "IMPORT_FAILED",
            "完整配置文档超过 32 MiB 安全上限。",
        ));
    }
    let value = serde_json::from_slice::<Value>(document)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("完整配置 JSON 无效：{error}")))?;
    reject_sensitive_configuration_fields(&value, "$")?;
    let parsed = serde_json::from_value::<ApplicationConfigurationDocument>(value)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("完整配置结构无效：{error}")))?;
    parsed.validate()?;
    Ok(parsed)
}

pub fn serialize_application_configuration(
    document: &ApplicationConfigurationDocument,
) -> AppResult<Vec<u8>> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|error| AppError::new("EXPORT_FAILED", format!("完整配置序列化失败：{error}")))?;
    let value = serde_json::from_slice::<Value>(&bytes).map_err(|error| {
        AppError::new("EXPORT_FAILED", format!("完整配置导出自检失败：{error}"))
    })?;
    reject_sensitive_configuration_fields(&value, "$")
        .map_err(|_| AppError::new("EXPORT_FAILED", "完整配置包含禁止导出的敏感或运行态字段。"))?;
    Ok(bytes)
}

/// 对未知字段也执行递归拒绝，避免 Serde 忽略攻击者额外塞入的秘密或运行态。
pub fn reject_sensitive_configuration_fields(value: &Value, path: &str) -> AppResult<()> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                if is_forbidden_field(&normalized) {
                    return Err(AppError::new(
                        "IMPORT_CONTAINS_SENSITIVE_DATA",
                        format!("配置文档禁止包含字段 {path}.{key}。"),
                    ));
                }
                reject_sensitive_configuration_fields(child, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                reject_sensitive_configuration_fields(child, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_forbidden_field(field: &str) -> bool {
    matches!(
        field,
        "password"
            | "password_bytes"
            | "private_key"
            | "private_key_pem"
            | "private_key_der"
            | "pkcs12"
            | "p12"
            | "secret_value"
            | "protected_blob"
            | "payload"
            | "request_payload"
            | "response_payload"
            | "selected_serial"
            | "device_serial"
            | "transport_id"
            | "runtime_state"
            | "active_profile_id"
            | "stats"
            | "resolved_address"
            | "resolved_routes"
            | "desktop_ip"
            | "adb_reverse_port"
            | "usb_device_port"
            | "control_socket"
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn document() -> ApplicationConfigurationDocument {
        let workspace = ProxyWorkspace::default();
        ApplicationConfigurationDocument {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id: workspace.id,
            workspaces: vec![workspace],
            settings: PortableSettings::from(&SettingsDraft::default()),
        }
    }

    #[test]
    fn full_configuration_round_trips() {
        let expected = document();
        let bytes = serialize_application_configuration(&expected).expect("serialize");
        assert_eq!(
            parse_application_configuration(&bytes).expect("parse"),
            expected
        );
    }

    #[test]
    fn sensitive_and_runtime_fields_are_rejected_before_deserialization() {
        for forbidden in [
            "private_key_pem",
            "password",
            "protected_blob",
            "selected_serial",
            "resolved_routes",
            "payload",
        ] {
            let mut value = serde_json::to_value(document()).expect("value");
            value
                .as_object_mut()
                .expect("object")
                .insert(forbidden.into(), json!("forbidden"));
            let error = parse_application_configuration(
                &serde_json::to_vec(&value).expect("document bytes"),
            )
            .expect_err("forbidden field must fail");
            assert_eq!(error.view_model.code, "IMPORT_CONTAINS_SENSITIVE_DATA");
        }
    }

    #[test]
    fn missing_selected_workspace_is_rejected() {
        let mut value = document();
        value.selected_workspace_id = WorkspaceId::new();
        assert!(value.validate().is_err());
    }
}
