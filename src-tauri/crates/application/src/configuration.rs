//! 整个应用的可移植配置文档。
//!
//! `.intercept-config` 与单个 `.intercept-workspace` 文档分工明确：前者用于完整备份与
//! 恢复，后者用于分享一个 Workspace。用户明确要求测试配置可在单个 JSON 文件中携带
//! 证书、PKCS#12 和密码；运行时仍只保存本机受保护引用。

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::document_security::{canonical_field_name, is_secret_field};
use specta::Type;

use crate::{
    AppError, AppResult, ChannelSettingsDraft, PortableCertificateMaterial, ProxyWorkspace,
    SettingsDraft, WorkspaceId, validate_certificate_materials,
};

pub const APPLICATION_CONFIGURATION_FORMAT_VERSION: u16 = 2;
pub const MAX_APPLICATION_CONFIGURATION_BYTES: usize = 128 * 1024 * 1024;
/// 监听证书只能引用应用受保护存储中的材料，不能携带文件路径或环境变量密码。
pub const MANAGED_LISTENER_CERTIFICATE_PREFIX: &str = "managed:listener-tls:";
/// Workspace 仅保留这个安装级符号引用；导出文档不携带对应 Root CA 私钥或叶子证书。
pub const INSTALLATION_ROOT_CERTIFICATE_REFERENCE: &str = "installation:root-ca";

/// 校验可移植文档中的证书引用边界。
///
/// Workspace 与完整配置导入都会调用此函数，因此 Tauri、未来 CLI/TUI 和测试夹具
/// 共享同一安全语义，不会出现“界面保存拒绝、文件导入却允许”的旁路。
pub fn validate_portable_certificate_references(workspace: &ProxyWorkspace) -> AppResult<()> {
    if let Some(reference) =
        workspace
            .certificate_references
            .iter()
            .find(|reference| match reference.kind {
                intercept_proxy_domain::CertificateReferenceKind::MitmRootCa => {
                    reference.reference != INSTALLATION_ROOT_CERTIFICATE_REFERENCE
                }
                _ => !reference
                    .reference
                    .starts_with(MANAGED_LISTENER_CERTIFICATE_PREFIX),
            })
    {
        return Err(AppError::new(
            "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED",
            "证书引用必须由应用内的原生导入功能创建，配置文档不能包含文件路径或外部密码引用。",
        )
        .entity(reference.id.to_string()));
    }
    Ok(())
}

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
    pub certificate_materials: Vec<PortableCertificateMaterial>,
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
            validate_portable_certificate_references(workspace)?;
        }
        if !ids.contains(&self.selected_workspace_id) {
            return Err(AppError::new(
                "APPLICATION_CONFIGURATION_INVALID",
                "当前选中的 Workspace 不存在于文档中。",
            )
            .entity(self.selected_workspace_id.to_string()));
        }
        validate_certificate_materials(&self.workspaces, &self.certificate_materials)?;
        Ok(())
    }
}

pub fn parse_application_configuration(
    document: &[u8],
) -> AppResult<ApplicationConfigurationDocument> {
    if document.len() > MAX_APPLICATION_CONFIGURATION_BYTES {
        return Err(AppError::new(
            "IMPORT_FAILED",
            "完整配置文档超过 128 MiB 安全上限。",
        ));
    }
    let value = serde_json::from_slice::<Value>(document)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("完整配置 JSON 无效：{error}")))?;
    reject_configuration_fields_outside_certificate_materials(&value)?;
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
    reject_configuration_fields_outside_certificate_materials(&value)
        .map_err(|_| AppError::new("EXPORT_FAILED", "完整配置包含禁止导出的敏感或运行态字段。"))?;
    Ok(bytes)
}

/// 证书载荷是唯一允许明文密码和 PKCS#12 的受控区域；其他未知字段继续递归拒绝。
fn reject_configuration_fields_outside_certificate_materials(value: &Value) -> AppResult<()> {
    let mut scanned = value.clone();
    if let Some(object) = scanned.as_object_mut() {
        object.insert("certificate_materials".into(), Value::Array(Vec::new()));
    }
    reject_sensitive_configuration_fields(&scanned, "$")
}

/// 对未知字段也执行递归拒绝，避免 Serde 忽略攻击者额外塞入的秘密或运行态。
pub fn reject_sensitive_configuration_fields(value: &Value, path: &str) -> AppResult<()> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if is_forbidden_field(key) {
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
    if is_secret_field(field) {
        return true;
    }
    matches!(
        canonical_field_name(field).as_str(),
        "payload"
            | "requestpayload"
            | "responsepayload"
            | "selectedserial"
            | "deviceserial"
            | "transportid"
            | "runtimestate"
            | "activeprofileid"
            | "stats"
            | "resolvedaddress"
            | "resolvedroutes"
            | "desktopip"
            | "adbreverseport"
            | "usbdeviceport"
            | "controlsocket"
    )
}

#[cfg(test)]
mod tests {
    use intercept_proxy_domain::{CertificateReference, CertificateReferenceKind};
    use serde_json::json;

    use super::*;

    fn document() -> ApplicationConfigurationDocument {
        let workspace = ProxyWorkspace::default();
        ApplicationConfigurationDocument {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id: workspace.id,
            workspaces: vec![workspace],
            settings: PortableSettings::from(&SettingsDraft::default()),
            certificate_materials: Vec::new(),
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
            "privateKey",
            "password",
            "basic_auth_password",
            "basicAuthPassword",
            "pkcs12_password",
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

    #[test]
    fn unmanaged_certificate_reference_is_rejected() {
        let mut value = document();
        value.workspaces[0]
            .certificate_references
            .push(CertificateReference {
                id: intercept_proxy_domain::CertificateReferenceId::new(),
                label: "外部文件".into(),
                kind: CertificateReferenceKind::UpstreamServerTrust,
                reference: "file:/tmp/server-ca.pem".into(),
            });

        let error = value.validate().expect_err("unmanaged reference must fail");
        assert_eq!(
            error.view_model.code,
            "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
        );
    }

    #[test]
    fn installation_root_reference_is_allowed_without_exporting_local_material() {
        let mut value = document();
        value.workspaces[0]
            .certificate_references
            .push(CertificateReference {
                id: intercept_proxy_domain::CertificateReferenceId::new(),
                label: "本机 MITM Root CA".into(),
                kind: CertificateReferenceKind::MitmRootCa,
                reference: INSTALLATION_ROOT_CERTIFICATE_REFERENCE.into(),
            });

        let bytes = serialize_application_configuration(&value).expect("serialize");
        assert_eq!(parse_application_configuration(&bytes).unwrap(), value);
    }
}
