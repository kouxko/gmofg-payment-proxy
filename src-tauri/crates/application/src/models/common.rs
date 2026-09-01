use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use uuid::Uuid;

pub use intercept_proxy_domain::{
    BodyCodecKind, BooleanPredicate, CertificateReference, CertificateReferenceId,
    CertificateReferenceKind, ChannelId, Condition, Document, DocumentMutation, DocumentPredicate,
    DocumentValue, DownstreamClientAuthentication, DownstreamTlsSettings, FixedServerSettings,
    ForwardProxyAuthentication, HttpBodyProcessing, HttpListenerSettings, JsonPointer,
    ListenerDataPlane, ListenerId, MitmSettings, NumberOperator, NumberPredicate,
    ProtocolDirection, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    ProxyListener, ProxyWorkspace, RuleContent, RuleDefinition, RuleDefinitionDraft, RuleStage,
    ScriptedSocketProcessing, SecretReference, SocketDownstreamSecurity,
    SocketDownstreamTlsSettings, SocketEndpoint, SocketLocalResponderTopology,
    SocketPayloadProcessing, SocketRelaySecurity, SocketRelaySettings, SocketRelayTopology,
    SocketTopology, SocketUpstreamTlsSettings, StringOperator, StringPredicate, UnifiedAction,
    UpstreamTlsSettings, WorkspaceId,
};

/// 标识一次代理启动周期。代理重启后旧周期的事件不得继续操作。
pub type RuntimeEpoch = Uuid;
/// 应用 DTO 使用的乐观并发版本号。
pub type Revision = u64;
/// 应用层会话标识。
pub type SessionId = Uuid;
/// 应用层规则标识。
pub type RuleId = Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 可复用于规则、设置和证书的字段校验结果。
pub struct FieldValidationViewModel {
    pub valid: bool,
    pub field_errors: BTreeMap<String, Vec<String>>,
    pub warnings: Vec<String>,
}

pub type RuleValidationViewModel = FieldValidationViewModel;
pub type CertificateValidationViewModel = FieldValidationViewModel;
pub type SettingsValidationViewModel = FieldValidationViewModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// 与具体组件库无关的视觉语义，前端只负责映射为 `HeroUI` 颜色。
pub enum UiTone {
    Neutral,
    Info,
    Positive,
    Warning,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Rust 判断某操作不可用时给出的稳定原因。
pub struct DisabledReason {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ChannelPresentationViewModel {
    pub id: ChannelId,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 通用分页请求；`normalized` 会把越界页码和页大小限制到安全范围。
pub struct PageRequest {
    pub page: u32,
    pub page_size: u32,
}

impl PageRequest {
    #[must_use]
    pub fn normalized(&self) -> Self {
        Self {
            page: self.page.max(1),
            page_size: self.page_size.clamp(1, 200),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MessageStage {
    TlsHandshake,
    Request,
    Response,
    Terminal,
}

impl MessageStage {
    pub fn display_zh(self) -> &'static str {
        match self {
            Self::TlsHandshake => "TLS 握手",
            Self::Request => "请求",
            Self::Response => "响应",
            Self::Terminal => "终态",
        }
    }
}
