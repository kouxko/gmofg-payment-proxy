//! 通用代理 Workspace 领域模型。
//!
//! Workspace 是桌面 UI、未来 TUI/CLI 和无界面测试共同使用的运行时配置边界。这里
//! 只保存可序列化配置与安全引用，不直接保存证书私钥、PKCS#12 密码、代理认证明文
//! 或文件内容。用户主动导出的单文件文档可以在 Workspace 外层附带 Listener TLS
//! 材料；运行时仍由 infrastructure 根据引用从系统受保护存储解析。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    AndroidNetworkProfile, CertificateReferenceId, DomainError, ErrorCode, ListenerId,
    ProtocolDocumentRuleDefinition, Revision, Rule, WorkspaceId,
};

mod listener_model;
mod socket_topology;
mod validation;

pub use listener_model::*;
pub use socket_topology::*;
pub use validation::{is_valid_socket_host, is_valid_upstream_origin};
use validation::{push_field_error, unique_ids, validate_listener, validate_workspace_references};

/// 首次启动创建的正向代理草稿端口。
/// 监听器默认禁用，因此不会在用户确认前打开端口。
pub const DEFAULT_FORWARD_PROXY_PORT: u16 = 8080;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CertificateReferenceKind {
    MitmRootCa,
    ReverseServerIdentity,
    DownstreamClientTrust,
    UpstreamClientIdentity,
    UpstreamServerTrust,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 证书材料的非敏感引用。实际证书链和私钥由 infrastructure 解析。
pub struct CertificateReference {
    pub id: CertificateReferenceId,
    pub label: String,
    pub kind: CertificateReferenceKind,
    pub reference: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ProxyWorkspace {
    pub id: WorkspaceId,
    pub name: String,
    pub revision: Revision,
    pub listeners: Vec<ProxyListener>,
    /// 规则通过 rule_* 用例维护，避免前端在 Workspace 表单中复制第二套规则编辑器。
    /// 字段仍属于领域聚合并参与导入导出，只不重复进入 Workspace 的 TypeScript DTO。
    #[specta(skip)]
    pub rules: Vec<Rule>,
    /// Schema 驱动的 协议报文规则；与 HTTP `rules` 使用完全独立的类型和维护入口。
    #[specta(skip)]
    pub protocol_rules: Vec<ProtocolDocumentRuleDefinition>,
    /// 协议报文规则 `created_order` 的单调高水位；删除规则不会降低此值。
    #[specta(skip)]
    pub protocol_rule_created_order_high_water: u64,
    pub certificate_references: Vec<CertificateReference>,
    /// 与该 Workspace 一起迁移的 Android 设备网络方案。
    /// 设备序列号、ADB transport、已解析桌面地址和运行态由宿主在启动时提供，
    /// 不属于此字段。
    pub android_network_profiles: Vec<AndroidNetworkProfile>,
}

impl Default for ProxyWorkspace {
    fn default() -> Self {
        Self {
            id: WorkspaceId::new(),
            name: "Untitled Workspace".into(),
            revision: Revision::INITIAL,
            listeners: vec![ProxyListener::default()],
            rules: Vec::new(),
            protocol_rules: Vec::new(),
            protocol_rule_created_order_high_water: 0,
            certificate_references: Vec::new(),
            android_network_profiles: Vec::new(),
        }
    }
}

impl ProxyWorkspace {
    /// 聚合全部字段错误，保证任何 Host 都得到相同校验结果。
    pub fn validate(&self) -> Result<(), DomainError> {
        let mut error = DomainError::new(ErrorCode::ConfigInvalid, "Workspace 配置存在字段错误");
        if self.name.trim().is_empty() {
            push_field_error(&mut error, "name", "Workspace 名称不能为空");
        }

        let certificate_ids = unique_ids(
            self.certificate_references.iter().map(|item| item.id),
            "certificate_references",
            &mut error,
        );
        let certificate_kinds = self
            .certificate_references
            .iter()
            .map(|item| (item.id, item.kind))
            .collect::<BTreeMap<_, _>>();
        for (index, reference) in self.certificate_references.iter().enumerate() {
            if reference.label.trim().is_empty() || reference.reference.trim().is_empty() {
                push_field_error(
                    &mut error,
                    format!("certificate_references.{index}"),
                    "证书名称和安全引用不能为空",
                );
            }
        }

        let listener_ids = unique_ids(
            self.listeners.iter().map(|listener| listener.id),
            "listeners",
            &mut error,
        );
        let mut enabled_endpoints = BTreeMap::new();
        for (index, listener) in self.listeners.iter().enumerate() {
            validate_listener(
                listener,
                index,
                &certificate_ids,
                &certificate_kinds,
                &mut error,
            );
            if listener.enabled {
                let endpoint = listener.bind_endpoint();
                if let Some(existing) = enabled_endpoints.insert(endpoint, index) {
                    push_field_error(
                        &mut error,
                        format!("listeners.{index}.port"),
                        format!("监听地址与 listeners.{existing} 重复"),
                    );
                }
            }
        }

        validate_workspace_references(self, &listener_ids, &mut error);

        if error.field_errors.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    }

    /// 乐观锁更新，校验失败时不会改变当前 Workspace。
    pub fn apply(
        &mut self,
        expected_revision: Revision,
        mut values: Self,
    ) -> Result<Revision, DomainError> {
        self.revision.verify(expected_revision)?;
        values.validate()?;
        let revision = self.revision.next();
        values.id = self.id;
        values.revision = revision;
        *self = values;
        Ok(revision)
    }
}

#[cfg(test)]
mod tests;
