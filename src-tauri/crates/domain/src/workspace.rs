//! 通用代理 Workspace 领域模型。
//!
//! Workspace 是桌面 UI、未来 TUI/CLI 和无界面测试共同使用的运行时配置边界。这里
//! 只保存可序列化配置与安全引用，不直接保存证书私钥、PKCS#12 密码、代理认证明文
//! 或文件内容。用户主动导出的单文件文档可以在 Workspace 外层附带 Listener TLS
//! 材料；运行时仍由 infrastructure 根据引用从系统受保护存储解析。

use std::collections::BTreeMap;

use crate::{
    AndroidNetworkProfile, CertificateReferenceId, ChannelId, DomainError, ErrorCode,
    HttpDocumentRuleContent, ListenerId, MessageStage, ProtocolDocumentRuleDefinition,
    ProtocolDocumentRuleId, ProtocolRuleStage, Revision, Rule, RuleContent, RuleDefinition,
    RuleDefinitionDraft, RuleId, RuleStage, SocketRuleContent, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

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
    /// HTTP 与 Socket 共用的唯一规则集合。编辑器通过统一 Rule 用例维护。
    #[specta(skip)]
    pub rule_definitions: Vec<RuleDefinition>,
    /// `created_order` 单调高水位；删除规则不会降低此值。
    #[specta(skip)]
    pub rule_created_order_high_water: u64,
    pub certificate_references: Vec<CertificateReference>,
    /// 与该 Workspace 一起迁移的 Android 设备网络方案。
    /// 设备序列号、ADB transport、已解析桌面地址和运行态由宿主在启动时提供，
    /// 不属于此字段。
    pub android_network_profiles: Vec<AndroidNetworkProfile>,
}

fn restore_document_rule(
    definition: &RuleDefinition,
    content: HttpDocumentRuleContent,
) -> Result<ProtocolDocumentRuleDefinition, DomainError> {
    ProtocolDocumentRuleDefinition::restore_from_unified(
        ProtocolDocumentRuleId::from_uuid(definition.rule_id().as_uuid()),
        definition.revision(),
        definition.name().to_owned(),
        definition.enabled(),
        definition.priority(),
        definition.created_order(),
        definition.listener_id(),
        content.package,
        protocol_stage_from_rule(definition.stage())?,
        content.conditions,
        content.actions,
    )
}

const fn message_stage_from_rule(stage: RuleStage) -> MessageStage {
    match stage {
        RuleStage::AppToProxy | RuleStage::ProxyToUpstream => MessageStage::Request,
        RuleStage::UpstreamToProxy | RuleStage::ProxyToApp => MessageStage::Response,
        RuleStage::TlsHandshake => MessageStage::TlsHandshake,
    }
}

fn runtime_priority(priority: i32) -> Result<u32, DomainError> {
    u32::try_from(priority)
        .map_err(|_| unified_persistence_error("priority", "HTTP 规则 priority 必须是非负整数"))
}

fn protocol_stage_from_rule(stage: RuleStage) -> Result<ProtocolRuleStage, DomainError> {
    match stage {
        RuleStage::AppToProxy => Ok(ProtocolRuleStage::AppToProxy),
        RuleStage::ProxyToUpstream => Ok(ProtocolRuleStage::ProxyToUpstream),
        RuleStage::UpstreamToProxy => Ok(ProtocolRuleStage::UpstreamToProxy),
        RuleStage::ProxyToApp => Ok(ProtocolRuleStage::ProxyToApp),
        RuleStage::TlsHandshake => Err(DomainError::new(
            ErrorCode::RuleInvalid,
            "Document 规则不能使用 TLS 握手阶段",
        )),
    }
}

const fn rule_stage_from_protocol(stage: ProtocolRuleStage) -> RuleStage {
    match stage {
        ProtocolRuleStage::AppToProxy => RuleStage::AppToProxy,
        ProtocolRuleStage::ProxyToUpstream => RuleStage::ProxyToUpstream,
        ProtocolRuleStage::UpstreamToProxy => RuleStage::UpstreamToProxy,
        ProtocolRuleStage::ProxyToApp => RuleStage::ProxyToApp,
    }
}

fn unified_persistence_error(field: &str, message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, "统一规则持久化数据无效")
        .with_field_error(field, message)
}

impl Default for ProxyWorkspace {
    fn default() -> Self {
        Self {
            id: WorkspaceId::new(),
            name: "Untitled Workspace".into(),
            revision: Revision::INITIAL,
            listeners: vec![ProxyListener::default()],
            rule_definitions: Vec::new(),
            rule_created_order_high_water: 0,
            certificate_references: Vec::new(),
            android_network_profiles: Vec::new(),
        }
    }
}

impl ProxyWorkspace {
    pub fn http_runtime_rule_execution_order(&self) -> Vec<RuleId> {
        let mut definitions = self
            .rule_definitions
            .iter()
            .filter(|definition| matches!(definition.content(), RuleContent::Http(_)))
            .collect::<Vec<_>>();
        definitions.sort_by_key(|definition| {
            let (direction, phase) = match definition.stage() {
                RuleStage::AppToProxy => (0, 0),
                RuleStage::ProxyToUpstream => (0, 1),
                RuleStage::UpstreamToProxy => (1, 0),
                RuleStage::ProxyToApp => (1, 1),
                RuleStage::TlsHandshake => (2, 0),
            };
            (
                direction,
                phase,
                definition.priority(),
                definition.created_order(),
                definition.rule_id(),
            )
        });
        definitions
            .into_iter()
            .map(RuleDefinition::rule_id)
            .collect()
    }

    pub fn http_runtime_rules(&self) -> Result<Vec<Rule>, DomainError> {
        let mut rules = Vec::new();
        for definition in &self.rule_definitions {
            let RuleContent::Http(content) = definition.content() else {
                continue;
            };
            if content.conditions.is_empty()
                && content.actions.is_empty()
                && content.document.is_none()
                && content.description.is_empty()
                && !content.one_shot
                && content.hit_count == 0
                && content.last_hit_at.is_none()
            {
                continue;
            }
            rules.push(Rule {
                id: definition.rule_id(),
                revision: definition.revision(),
                name: definition.name().to_owned(),
                description: content.description.clone(),
                enabled: definition.enabled(),
                priority: runtime_priority(definition.priority())?,
                created_order: definition.created_order(),
                channel: Some(ChannelId::new(definition.listener_id().to_string())?),
                stage: message_stage_from_rule(definition.stage()),
                conditions: content.conditions.clone(),
                actions: content.actions.clone(),
                one_shot: content.one_shot,
                hit_count: content.hit_count,
                last_hit_at: content.last_hit_at,
            });
        }
        Ok(rules)
    }

    pub fn replace_http_runtime_rules(&mut self, rules: Vec<Rule>) -> Result<(), DomainError> {
        let mut preserved = self
            .rule_definitions
            .iter()
            .filter(|definition| match definition.content() {
                RuleContent::Socket(_) => true,
                RuleContent::Http(content) => content.document.is_some(),
            })
            .cloned()
            .collect::<Vec<_>>();
        for rule in rules {
            let listener_id = listener_id_from_channel(rule.channel.as_ref())?;
            let existing = self
                .rule_definitions
                .iter()
                .find(|definition| definition.rule_id() == rule.id)
                .cloned();
            let document = existing
                .as_ref()
                .and_then(|definition| match definition.content() {
                    RuleContent::Http(content) => content.document.clone(),
                    RuleContent::Socket(_) => None,
                });
            let stage = existing.as_ref().map_or_else(
                || rule_stage_from_message(rule.stage),
                RuleDefinition::stage,
            );
            let priority = existing.as_ref().map_or_else(
                || {
                    i32::try_from(rule.priority).map_err(|_| {
                        unified_persistence_error("priority", "HTTP 规则 priority 超出统一规则范围")
                    })
                },
                |definition| Ok(definition.priority()),
            )?;
            preserved.retain(|definition| definition.rule_id() != rule.id);
            preserved.push(RuleDefinition::restore(
                rule.id,
                rule.revision,
                crate::RuleDefinitionDraft {
                    name: rule.name,
                    enabled: rule.enabled,
                    priority,
                    listener_id,
                    stage,
                    content: RuleContent::Http(crate::HttpRuleContent {
                        description: rule.description,
                        conditions: rule.conditions,
                        actions: rule.actions,
                        document,
                        one_shot: rule.one_shot,
                        hit_count: rule.hit_count,
                        last_hit_at: rule.last_hit_at,
                    }),
                },
                rule.created_order,
            )?);
        }
        crate::sort_rule_definitions(&mut preserved);
        self.rule_created_order_high_water = self.rule_created_order_high_water.max(
            preserved
                .iter()
                .map(RuleDefinition::created_order)
                .max()
                .unwrap_or(0),
        );
        self.rule_definitions = preserved;
        Ok(())
    }

    pub fn document_runtime_rules(
        &self,
    ) -> Result<Vec<ProtocolDocumentRuleDefinition>, DomainError> {
        let mut rules = Vec::new();
        for definition in &self.rule_definitions {
            let document = match definition.content() {
                RuleContent::Http(content) => content.document.clone(),
                RuleContent::Socket(content) => Some(HttpDocumentRuleContent {
                    package: content.package.clone(),
                    conditions: content.conditions.clone(),
                    actions: content.actions.clone(),
                }),
            };
            if let Some(document) = document {
                rules.push(restore_document_rule(definition, document)?);
            }
        }
        Ok(rules)
    }

    /// Replaces the document projection while keeping the unified collection authoritative.
    ///
    /// HTTP definitions retain their HTTP conditions/actions and only replace the embedded
    /// document portion. Socket definitions are rebuilt from the document projection.
    pub fn replace_document_runtime_rules(
        &mut self,
        rules: Vec<ProtocolDocumentRuleDefinition>,
    ) -> Result<(), DomainError> {
        let mut definitions = self
            .rule_definitions
            .iter()
            .filter(|definition| match definition.content() {
                RuleContent::Http(content) => content.document.is_none(),
                RuleContent::Socket(_) => false,
            })
            .cloned()
            .collect::<Vec<_>>();

        for rule in rules {
            let rule_id = RuleId::from_uuid(rule.rule_id().as_uuid());
            let existing_http = self.rule_definitions.iter().find_map(|definition| {
                (definition.rule_id() == rule_id).then(|| match definition.content() {
                    RuleContent::Http(content) => Some(content.clone()),
                    RuleContent::Socket(_) => None,
                })?
            });
            let document = HttpDocumentRuleContent {
                package: rule.package().clone(),
                conditions: rule.conditions().to_vec(),
                actions: rule.actions().to_vec(),
            };
            let content = if let Some(mut http) = existing_http {
                http.document = Some(document);
                RuleContent::Http(http)
            } else {
                RuleContent::Socket(SocketRuleContent {
                    package: document.package,
                    conditions: document.conditions,
                    actions: document.actions,
                })
            };
            definitions.push(RuleDefinition::restore(
                rule_id,
                rule.revision(),
                RuleDefinitionDraft {
                    name: rule.name().to_owned(),
                    enabled: rule.enabled(),
                    priority: rule.priority(),
                    listener_id: rule.listener_id(),
                    stage: rule_stage_from_protocol(rule.stage()),
                    content,
                },
                rule.created_order(),
            )?);
        }
        crate::sort_rule_definitions(&mut definitions);
        self.rule_created_order_high_water = self.rule_created_order_high_water.max(
            definitions
                .iter()
                .map(RuleDefinition::created_order)
                .max()
                .unwrap_or(0),
        );
        self.rule_definitions = definitions;
        Ok(())
    }

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

fn listener_id_from_channel(channel: Option<&ChannelId>) -> Result<ListenerId, DomainError> {
    let channel = channel.ok_or_else(|| {
        unified_persistence_error("listener_id", "HTTP 规则必须绑定单个 Listener")
    })?;
    Uuid::parse_str(channel.as_str())
        .map(ListenerId::from_uuid)
        .map_err(|_| unified_persistence_error("listener_id", "HTTP 规则 Listener ID 无效"))
}

const fn rule_stage_from_message(stage: MessageStage) -> RuleStage {
    match stage {
        MessageStage::Request => RuleStage::ProxyToUpstream,
        MessageStage::Response => RuleStage::ProxyToApp,
        MessageStage::TlsHandshake => RuleStage::TlsHandshake,
    }
}

#[cfg(test)]
mod tests;
