use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    CertificateReference, CertificateReferenceId, ChannelId, DisabledReason, Revision, UiTone,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CertificateItemViewModel {
    pub kind: String,
    pub subject: String,
    pub usage: String,
    pub sans: Vec<String>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub sha256_fingerprint: String,
    pub status_text: String,
    pub ui_tone: UiTone,
}

/// 代理监听页面使用的证书引用详情。
/// Workspace 只保存安全引用；证书的主题、SAN、有效期和指纹必须由 Rust 重新解析。
/// 单个引用失效时保留该行并返回 `error_message`，避免一份坏证书阻塞其他证书展示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ListenerCertificateDetailViewModel {
    pub reference_id: CertificateReferenceId,
    pub label: String,
    pub certificate: Option<CertificateItemViewModel>,
    pub error_message: Option<String>,
}

/// 原生导入成功后的安全引用与已解析详情。
/// 导入文件内容、私钥和密码都不会进入 IPC；前端只获得可持久化引用及公开证书元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ListenerCertificateImportViewModel {
    pub reference: CertificateReference,
    pub detail: ListenerCertificateDetailViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CertificateOverviewViewModel {
    pub revision: Revision,
    pub ready: bool,
    pub status_text: String,
    pub ui_tone: UiTone,
    pub items: Vec<CertificateItemViewModel>,
    pub can_initialize: bool,
    pub can_change: bool,
    pub disabled_reason: Option<DisabledReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 设置页中的单个产品通道草稿。
pub struct ChannelSettingsDraft {
    pub id: ChannelId,
    pub display_name: String,
    pub enabled: bool,
    pub port: u16,
    pub upstream_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 设置页提交的前端友好草稿，端口会再转换并执行领域校验。
pub struct SettingsDraft {
    pub expected_revision: Option<Revision>,
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

impl Default for SettingsDraft {
    fn default() -> Self {
        Self {
            expected_revision: None,
            bind_address: "0.0.0.0".into(),
            channels: Vec::new(),
            connect_timeout_seconds: 70,
            write_timeout_seconds: 70,
            read_timeout_seconds: 70,
            rewrite_host: true,
            max_body_bytes: 4 * 1024 * 1024,
            max_sessions: 500,
            max_memory_bytes: 256 * 1024 * 1024,
            leaf_sans: Vec::new(),
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 设置页展示模型。
pub struct SettingsViewModel {
    pub stored: SettingsDraft,
    pub revision: Revision,
    pub can_write: bool,
    pub disabled_reason: Option<DisabledReason>,
    pub fixed_tls_version: String,
    pub redirects_enabled: bool,
    pub retries_enabled: bool,
    pub payload_policy_text: String,
}
