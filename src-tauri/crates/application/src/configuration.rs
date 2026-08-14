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
    AppError, AppResult, ChannelSettingsDraft, PortableApplicationProtocolPackage,
    PortableCertificateMaterial, ProxyWorkspace, ProxyWorkspaceV2, SettingsDraft, WorkspaceId,
    validate_certificate_materials, validate_configuration_package_references,
};

pub const APPLICATION_CONFIGURATION_FORMAT_VERSION: u16 = 4;
pub const APPLICATION_CONFIGURATION_V3_FORMAT_VERSION: u16 = 3;
pub const APPLICATION_CONFIGURATION_V2_FORMAT_VERSION: u16 = 2;
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
            concat!(
                "证书引用必须由应用内的原生导入功能创建，",
                "配置文档不能包含文件路径或外部密码引用。"
            ),
        )
        .entity(reference.id.to_string()));
    }
    Ok(())
}

/// 不含乐观锁 revision 的全局设置值。
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
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
    pub protocol_packages: Vec<PortableApplicationProtocolPackage>,
}

/// v3 完整配置支持显式 Socket 拓扑，但没有协议包载荷或 Socket 规则 wire。
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicationConfigurationDocumentV3 {
    pub format_version: u16,
    pub selected_workspace_id: WorkspaceId,
    pub workspaces: Vec<ProxyWorkspace>,
    pub settings: PortableSettings,
    pub certificate_materials: Vec<PortableCertificateMaterial>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationConfigurationDocumentV2 {
    pub format_version: u16,
    pub selected_workspace_id: WorkspaceId,
    pub workspaces: Vec<ProxyWorkspaceV2>,
    pub settings: PortableSettings,
    pub certificate_materials: Vec<PortableCertificateMaterial>,
}

impl TryFrom<ApplicationConfigurationDocumentV2> for ApplicationConfigurationDocument {
    type Error = AppError;

    fn try_from(value: ApplicationConfigurationDocumentV2) -> Result<Self, Self::Error> {
        if value.format_version != APPLICATION_CONFIGURATION_V2_FORMAT_VERSION {
            return Err(unsupported_configuration_version(value.format_version));
        }
        Ok(Self {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id: value.selected_workspace_id,
            workspaces: value.workspaces.into_iter().map(Into::into).collect(),
            settings: value.settings,
            certificate_materials: value.certificate_materials,
            protocol_packages: Vec::new(),
        })
    }
}

impl TryFrom<ApplicationConfigurationDocumentV3> for ApplicationConfigurationDocument {
    type Error = AppError;

    fn try_from(value: ApplicationConfigurationDocumentV3) -> Result<Self, Self::Error> {
        if value.format_version != APPLICATION_CONFIGURATION_V3_FORMAT_VERSION {
            return Err(unsupported_configuration_version(value.format_version));
        }
        Ok(Self {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id: value.selected_workspace_id,
            workspaces: value.workspaces,
            settings: value.settings,
            certificate_materials: value.certificate_materials,
            protocol_packages: Vec::new(),
        })
    }
}

/// 已迁移到当前模型的完整配置及其原始 wire 版本。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedApplicationConfigurationDocument {
    pub source_version: u16,
    pub document: ApplicationConfigurationDocument,
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
        self.validate_common()?;
        validate_configuration_package_references(&self.workspaces, &self.protocol_packages, true)
    }

    fn validate_common(&self) -> AppResult<()> {
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
    Ok(parse_application_configuration_with_source(document)?.document)
}

/// 解析完整配置并保留来源版本，供恢复用例区分 legacy 文档。
pub fn parse_application_configuration_with_source(
    document: &[u8],
) -> AppResult<ParsedApplicationConfigurationDocument> {
    if document.len() > MAX_APPLICATION_CONFIGURATION_BYTES {
        return Err(AppError::new(
            "IMPORT_FAILED",
            "完整配置文档超过 128 MiB 安全上限。",
        ));
    }
    let value = serde_json::from_slice::<Value>(document)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("完整配置 JSON 无效：{error}")))?;
    reject_configuration_fields_outside_certificate_materials(&value)?;
    let version = read_configuration_format_version(&value)?;
    if version == APPLICATION_CONFIGURATION_V3_FORMAT_VERSION {
        crate::portable_socket_rules::reject_configuration_fields(&value)?;
    }
    let parsed = match version {
        APPLICATION_CONFIGURATION_FORMAT_VERSION => {
            serde_json::from_value::<ApplicationConfigurationDocument>(value)
                .map_err(|error| invalid_configuration_structure(&error))?
        }
        APPLICATION_CONFIGURATION_V3_FORMAT_VERSION => {
            let legacy = serde_json::from_value::<ApplicationConfigurationDocumentV3>(value)
                .map_err(|error| invalid_configuration_structure(&error))?;
            legacy.try_into()?
        }
        APPLICATION_CONFIGURATION_V2_FORMAT_VERSION => {
            let legacy = serde_json::from_value::<ApplicationConfigurationDocumentV2>(value)
                .map_err(|error| invalid_configuration_structure(&error))?;
            legacy.try_into()?
        }
        _ => return Err(unsupported_configuration_version(version)),
    };
    parsed.validate_common()?;
    validate_configuration_package_references(
        &parsed.workspaces,
        &parsed.protocol_packages,
        version == APPLICATION_CONFIGURATION_FORMAT_VERSION,
    )?;
    Ok(ParsedApplicationConfigurationDocument {
        source_version: version,
        document: parsed,
    })
}

fn read_configuration_format_version(value: &Value) -> AppResult<u16> {
    value
        .get("format_version")
        .and_then(Value::as_u64)
        .and_then(|version| u16::try_from(version).ok())
        .ok_or_else(|| AppError::new("IMPORT_FAILED", "完整配置 format_version 缺失或无效。"))
}

fn invalid_configuration_structure(error: &serde_json::Error) -> AppError {
    AppError::new("IMPORT_FAILED", format!("完整配置结构无效：{error}"))
}

fn unsupported_configuration_version(version: u16) -> AppError {
    AppError::new(
        "APPLICATION_CONFIGURATION_VERSION_UNSUPPORTED",
        format!(
            "配置文档版本 {version} 不受支持；当前支持版本 2、3 和 \
             {APPLICATION_CONFIGURATION_FORMAT_VERSION}。"
        ),
    )
}

pub fn serialize_application_configuration(
    document: &ApplicationConfigurationDocument,
) -> AppResult<Vec<u8>> {
    document.validate()?;
    let value = serde_json::to_value(document)
        .map_err(|error| AppError::new("EXPORT_FAILED", format!("完整配置序列化失败：{error}")))?;
    reject_configuration_fields_outside_certificate_materials(&value)
        .map_err(|_| AppError::new("EXPORT_FAILED", "完整配置包含禁止导出的敏感或运行态字段。"))?;
    serde_json::to_vec_pretty(&value)
        .map_err(|error| AppError::new("EXPORT_FAILED", format!("完整配置序列化失败：{error}")))
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
mod tests;
