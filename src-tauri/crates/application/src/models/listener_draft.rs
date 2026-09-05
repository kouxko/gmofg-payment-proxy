//! Listener 编辑态 DTO。
//!
//! 严格领域配置始终保持可运行不变量；本 DTO 只把 UI 操作期间允许暂缺的证书与协议包
//! 引用表示为 `Option`，其余字段继续复用领域强类型。

mod normalization;
mod transition;

use intercept_proxy_domain::{
    BodyCodecKind, CertificateReferenceId, DownstreamClientAuthentication, DownstreamTlsSettings,
    ForwardProxyAuthentication, HttpBodyProcessing, HttpListenerSettings, HttpTopology,
    ListenerDataPlane, ListenerId, MitmSettings, ProtocolPackageRef, ProxyListener, ProxyWorkspace,
    SocketDownstreamSecurity, SocketDownstreamTlsSettings, SocketEndpoint, SocketPayloadProcessing,
    SocketRelaySecurity, SocketRelaySettings, SocketRuntimeLimits, SocketTopology,
    SocketUpstreamTlsSettings,
};
use serde::{Deserialize, Serialize};
use specta::Type;

pub use transition::{
    HttpResponseMode, ListenerDraftTransition, SocketProcessingMode, SocketTopologyMode,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ListenerDraft {
    pub id: ListenerId,
    pub name: String,
    pub enabled: bool,
    pub bind_address: String,
    pub port: u16,
    pub connect_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub write_timeout_ms: u64,
    pub data_plane: ListenerDataPlaneDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(
    tag = "kind",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ListenerDataPlaneDraft {
    Http(HttpListenerDraftSettings),
    Socket(SocketRelayDraftSettings),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct HttpListenerDraftSettings {
    pub authentication: ForwardProxyAuthentication,
    pub mitm: MitmSettings,
    pub downstream_tls: DownstreamTlsDraftSettings,
    pub request_body_codec: BodyCodecKind,
    pub response_body_codec: BodyCodecKind,
    pub body_processing: HttpBodyProcessing,
    pub topology: HttpTopology,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct DownstreamTlsDraftSettings {
    pub enabled: bool,
    pub server_identity: Option<CertificateReferenceId>,
    pub dynamic_sni_allowlist: Vec<String>,
    pub client_authentication: DownstreamClientAuthenticationDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum DownstreamClientAuthenticationDraft {
    Disabled,
    Optional {
        trust: Option<CertificateReferenceId>,
    },
    Required {
        trust: Option<CertificateReferenceId>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRelayDraftSettings {
    pub topology: SocketTopologyDraft,
    pub maximum_connections: u16,
    pub runtime_limits: SocketRuntimeLimits,
    pub processing: SocketPayloadProcessingDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(
    tag = "mode",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SocketTopologyDraft {
    Relay(SocketRelayTopologyDraft),
    LocalResponder(SocketLocalResponderTopologyDraft),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRelayTopologyDraft {
    pub upstream: SocketEndpoint,
    pub security: SocketRelaySecurityDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketLocalResponderTopologyDraft {
    pub downstream_security: SocketDownstreamSecurityDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum SocketRelaySecurityDraft {
    Transparent,
    TcpToTls {
        upstream_tls: SocketUpstreamTlsSettings,
    },
    TlsToTcp {
        downstream_tls: SocketDownstreamTlsDraftSettings,
    },
    TlsToTls {
        downstream_tls: SocketDownstreamTlsDraftSettings,
        upstream_tls: SocketUpstreamTlsSettings,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum SocketDownstreamSecurityDraft {
    Tcp,
    Tls {
        downstream_tls: SocketDownstreamTlsDraftSettings,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketDownstreamTlsDraftSettings {
    pub server_identity: Option<CertificateReferenceId>,
    pub client_authentication: DownstreamClientAuthenticationDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(
    tag = "mode",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SocketPayloadProcessingDraft {
    Direct,
    Scripted(ScriptedSocketProcessingDraft),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ScriptedSocketProcessingDraft {
    pub package: Option<ProtocolPackageRef>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct ListenerDraftValidationViewModel {
    pub valid: bool,
    pub normalized: Option<ProxyWorkspace>,
    pub field_errors: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for HttpListenerDraftSettings {
    fn default() -> Self {
        HttpListenerSettings::default().into()
    }
}

impl Default for SocketRelayDraftSettings {
    fn default() -> Self {
        SocketRelaySettings::default().into()
    }
}

impl From<ProxyListener> for ListenerDraft {
    fn from(value: ProxyListener) -> Self {
        Self {
            id: value.id,
            name: value.name,
            enabled: value.enabled,
            bind_address: value.bind_address,
            port: value.port,
            connect_timeout_ms: value.connect_timeout_ms,
            read_timeout_ms: value.read_timeout_ms,
            write_timeout_ms: value.write_timeout_ms,
            data_plane: value.data_plane.into(),
        }
    }
}

impl From<ListenerDataPlane> for ListenerDataPlaneDraft {
    fn from(value: ListenerDataPlane) -> Self {
        match value {
            ListenerDataPlane::Http(settings) => Self::Http(settings.into()),
            ListenerDataPlane::Socket(settings) => Self::Socket(settings.into()),
        }
    }
}

impl From<HttpListenerSettings> for HttpListenerDraftSettings {
    fn from(value: HttpListenerSettings) -> Self {
        Self {
            authentication: value.authentication,
            mitm: value.mitm,
            downstream_tls: value.downstream_tls.into(),
            request_body_codec: value.request_body_codec,
            response_body_codec: value.response_body_codec,
            body_processing: value.body_processing,
            topology: value.topology,
        }
    }
}

impl From<DownstreamTlsSettings> for DownstreamTlsDraftSettings {
    fn from(value: DownstreamTlsSettings) -> Self {
        Self {
            enabled: value.enabled,
            server_identity: value.server_identity,
            dynamic_sni_allowlist: value.dynamic_sni_allowlist,
            client_authentication: value.client_authentication.into(),
        }
    }
}

impl From<DownstreamClientAuthentication> for DownstreamClientAuthenticationDraft {
    fn from(value: DownstreamClientAuthentication) -> Self {
        match value {
            DownstreamClientAuthentication::Disabled => Self::Disabled,
            DownstreamClientAuthentication::Optional { trust } => {
                Self::Optional { trust: Some(trust) }
            }
            DownstreamClientAuthentication::Required { trust } => {
                Self::Required { trust: Some(trust) }
            }
        }
    }
}

impl From<SocketRelaySettings> for SocketRelayDraftSettings {
    fn from(value: SocketRelaySettings) -> Self {
        Self {
            topology: value.topology.into(),
            maximum_connections: value.maximum_connections,
            runtime_limits: value.runtime_limits,
            processing: value.processing.into(),
        }
    }
}

impl From<SocketTopology> for SocketTopologyDraft {
    fn from(value: SocketTopology) -> Self {
        match value {
            SocketTopology::Relay(value) => Self::Relay(SocketRelayTopologyDraft {
                upstream: value.upstream,
                security: value.security.into(),
            }),
            SocketTopology::LocalResponder(value) => {
                Self::LocalResponder(SocketLocalResponderTopologyDraft {
                    downstream_security: value.downstream_security.into(),
                })
            }
        }
    }
}

impl From<SocketRelaySecurity> for SocketRelaySecurityDraft {
    fn from(value: SocketRelaySecurity) -> Self {
        match value {
            SocketRelaySecurity::Transparent => Self::Transparent,
            SocketRelaySecurity::TcpToTls { upstream_tls } => Self::TcpToTls { upstream_tls },
            SocketRelaySecurity::TlsToTcp { downstream_tls } => Self::TlsToTcp {
                downstream_tls: downstream_tls.into(),
            },
            SocketRelaySecurity::TlsToTls {
                downstream_tls,
                upstream_tls,
            } => Self::TlsToTls {
                downstream_tls: downstream_tls.into(),
                upstream_tls,
            },
        }
    }
}

impl From<SocketDownstreamSecurity> for SocketDownstreamSecurityDraft {
    fn from(value: SocketDownstreamSecurity) -> Self {
        match value {
            SocketDownstreamSecurity::Tcp => Self::Tcp,
            SocketDownstreamSecurity::Tls { downstream_tls } => Self::Tls {
                downstream_tls: downstream_tls.into(),
            },
        }
    }
}

impl From<SocketDownstreamTlsSettings> for SocketDownstreamTlsDraftSettings {
    fn from(value: SocketDownstreamTlsSettings) -> Self {
        Self {
            server_identity: Some(value.server_identity),
            client_authentication: value.client_authentication.into(),
        }
    }
}

impl From<SocketPayloadProcessing> for SocketPayloadProcessingDraft {
    fn from(value: SocketPayloadProcessing) -> Self {
        match value {
            SocketPayloadProcessing::Direct => Self::Direct,
            SocketPayloadProcessing::Scripted(value) => {
                Self::Scripted(ScriptedSocketProcessingDraft {
                    package: Some(value.package),
                })
            }
        }
    }
}
