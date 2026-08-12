//! Listener 公共配置与互斥的数据平面配置。

use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

use crate::{CertificateReferenceId, ListenerId};

use super::DEFAULT_FORWARD_PROXY_PORT;

pub const DEFAULT_SOCKET_MAXIMUM_CONNECTIONS: u16 = 500;
pub const MAX_SOCKET_MAXIMUM_CONNECTIONS: u16 = 5_000;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BodyCodecKind {
    #[default]
    Auto,
    Raw,
    Utf8,
    ShiftJis,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SecretReference {
    pub provider: String,
    pub key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ForwardProxyAuthentication {
    None,
    Basic { credential: SecretReference },
}

impl<'de> Deserialize<'de> for ForwardProxyAuthentication {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            None {},
            Basic { credential: SecretReference },
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::None {} => Self::None,
            Wire::Basic { credential } => Self::Basic { credential },
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct MitmSettings {
    pub enabled: bool,
    pub authority_allowlist: Vec<String>,
    pub root_ca: Option<CertificateReferenceId>,
    pub maximum_cached_leaf_certificates: u16,
}

impl Default for MitmSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            authority_allowlist: Vec::new(),
            root_ca: None,
            maximum_cached_leaf_certificates: 256,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum DownstreamClientAuthentication {
    Disabled,
    Optional { trust: CertificateReferenceId },
    Required { trust: CertificateReferenceId },
}

impl<'de> Deserialize<'de> for DownstreamClientAuthentication {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Disabled {},
            Optional { trust: CertificateReferenceId },
            Required { trust: CertificateReferenceId },
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Disabled {} => Self::Disabled,
            Wire::Optional { trust } => Self::Optional { trust },
            Wire::Required { trust } => Self::Required { trust },
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct DownstreamTlsSettings {
    pub enabled: bool,
    pub server_identity: Option<CertificateReferenceId>,
    #[serde(default)]
    pub dynamic_sni_allowlist: Vec<String>,
    pub client_authentication: DownstreamClientAuthentication,
}

impl Default for DownstreamTlsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            server_identity: None,
            dynamic_sni_allowlist: Vec::new(),
            client_authentication: DownstreamClientAuthentication::Disabled,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct UpstreamTlsSettings {
    pub verify_hostname: bool,
    pub server_trust: Option<CertificateReferenceId>,
    pub client_identity: Option<CertificateReferenceId>,
}

impl Default for UpstreamTlsSettings {
    fn default() -> Self {
        Self {
            verify_hostname: true,
            server_trust: None,
            client_identity: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct FixedServerSettings {
    pub upstream_url: String,
    pub upstream_tls: UpstreamTlsSettings,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct HttpListenerSettings {
    pub authentication: ForwardProxyAuthentication,
    pub mitm: MitmSettings,
    pub downstream_tls: DownstreamTlsSettings,
    pub request_body_codec: BodyCodecKind,
    pub response_body_codec: BodyCodecKind,
    pub fixed_server: Option<FixedServerSettings>,
}

impl Default for HttpListenerSettings {
    fn default() -> Self {
        Self {
            authentication: ForwardProxyAuthentication::None,
            mitm: MitmSettings::default(),
            downstream_tls: DownstreamTlsSettings::default(),
            request_body_codec: BodyCodecKind::Auto,
            response_body_codec: BodyCodecKind::Auto,
            fixed_server: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketDownstreamTlsSettings {
    pub server_identity: CertificateReferenceId,
    pub client_authentication: DownstreamClientAuthentication,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketUpstreamTlsSettings {
    pub verify_hostname: bool,
    pub server_trust: Option<CertificateReferenceId>,
    pub client_identity: Option<CertificateReferenceId>,
}

impl Default for SocketUpstreamTlsSettings {
    fn default() -> Self {
        Self {
            verify_hostname: true,
            server_trust: None,
            client_identity: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SocketRelaySecurity {
    Transparent,
    TcpToTls {
        upstream_tls: SocketUpstreamTlsSettings,
    },
    TlsToTcp {
        downstream_tls: SocketDownstreamTlsSettings,
    },
    TlsToTls {
        downstream_tls: SocketDownstreamTlsSettings,
        upstream_tls: SocketUpstreamTlsSettings,
    },
}

impl<'de> Deserialize<'de> for SocketRelaySecurity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Transparent {},
            TcpToTls {
                upstream_tls: SocketUpstreamTlsSettings,
            },
            TlsToTcp {
                downstream_tls: SocketDownstreamTlsSettings,
            },
            TlsToTls {
                downstream_tls: SocketDownstreamTlsSettings,
                upstream_tls: SocketUpstreamTlsSettings,
            },
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Transparent {} => Self::Transparent,
            Wire::TcpToTls { upstream_tls } => Self::TcpToTls { upstream_tls },
            Wire::TlsToTcp { downstream_tls } => Self::TlsToTcp { downstream_tls },
            Wire::TlsToTls {
                downstream_tls,
                upstream_tls,
            } => Self::TlsToTls {
                downstream_tls,
                upstream_tls,
            },
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRelaySettings {
    pub upstream: SocketEndpoint,
    pub security: SocketRelaySecurity,
    pub maximum_connections: u16,
}

impl Default for SocketRelaySettings {
    fn default() -> Self {
        Self {
            upstream: SocketEndpoint::default(),
            security: SocketRelaySecurity::Transparent,
            maximum_connections: DEFAULT_SOCKET_MAXIMUM_CONNECTIONS,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(
    tag = "kind",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ListenerDataPlane {
    Http(HttpListenerSettings),
    Socket(SocketRelaySettings),
}

impl Default for ListenerDataPlane {
    fn default() -> Self {
        Self::Http(HttpListenerSettings::default())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ProxyListener {
    pub id: ListenerId,
    pub name: String,
    pub enabled: bool,
    pub bind_address: String,
    pub port: u16,
    pub allowed_client_cidrs: Vec<String>,
    pub connect_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub write_timeout_ms: u64,
    pub data_plane: ListenerDataPlane,
}

impl Default for ProxyListener {
    fn default() -> Self {
        Self {
            id: ListenerId::new(),
            name: "默认代理监听".into(),
            enabled: false,
            bind_address: "127.0.0.1".into(),
            port: DEFAULT_FORWARD_PROXY_PORT,
            allowed_client_cidrs: Vec::new(),
            connect_timeout_ms: 30_000,
            read_timeout_ms: 70_000,
            write_timeout_ms: 70_000,
            data_plane: ListenerDataPlane::default(),
        }
    }
}

impl ProxyListener {
    #[must_use]
    pub const fn id(&self) -> ListenerId {
        self.id
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub fn bind_endpoint(&self) -> (&str, u16) {
        (&self.bind_address, self.port)
    }

    #[must_use]
    pub const fn http(&self) -> Option<&HttpListenerSettings> {
        match &self.data_plane {
            ListenerDataPlane::Http(settings) => Some(settings),
            ListenerDataPlane::Socket(_) => None,
        }
    }

    #[must_use]
    pub const fn http_mut(&mut self) -> Option<&mut HttpListenerSettings> {
        match &mut self.data_plane {
            ListenerDataPlane::Http(settings) => Some(settings),
            ListenerDataPlane::Socket(_) => None,
        }
    }

    #[must_use]
    pub const fn socket(&self) -> Option<&SocketRelaySettings> {
        match &self.data_plane {
            ListenerDataPlane::Http(_) => None,
            ListenerDataPlane::Socket(settings) => Some(settings),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyListenerV2 {
    pub id: ListenerId,
    pub name: String,
    pub enabled: bool,
    pub bind_address: String,
    pub port: u16,
    pub authentication: ForwardProxyAuthentication,
    pub allowed_client_cidrs: Vec<String>,
    pub mitm: MitmSettings,
    pub connect_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub write_timeout_ms: u64,
    #[serde(default)]
    pub downstream_tls: Option<DownstreamTlsSettings>,
    #[serde(default)]
    pub request_body_codec: BodyCodecKind,
    #[serde(default)]
    pub response_body_codec: BodyCodecKind,
    pub fixed_server: Option<FixedServerSettings>,
}

impl From<ProxyListenerV2> for ProxyListener {
    fn from(value: ProxyListenerV2) -> Self {
        Self {
            id: value.id,
            name: value.name,
            enabled: value.enabled,
            bind_address: value.bind_address,
            port: value.port,
            allowed_client_cidrs: value.allowed_client_cidrs,
            connect_timeout_ms: value.connect_timeout_ms,
            read_timeout_ms: value.read_timeout_ms,
            write_timeout_ms: value.write_timeout_ms,
            data_plane: ListenerDataPlane::Http(HttpListenerSettings {
                authentication: value.authentication,
                mitm: value.mitm,
                downstream_tls: value.downstream_tls.unwrap_or_default(),
                request_body_codec: value.request_body_codec,
                response_body_codec: value.response_body_codec,
                fixed_server: value.fixed_server,
            }),
        }
    }
}
