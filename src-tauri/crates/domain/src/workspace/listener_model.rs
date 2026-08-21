//! Listener 公共配置与互斥的数据平面配置。

use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

use crate::{CertificateReferenceId, ListenerId, ProtocolPackageRef};

use super::{DEFAULT_FORWARD_PROXY_PORT, SocketRelaySettings};

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

/// HTTP Body 的处理方式。
///
/// `Plain` 保持现有 HTTP 语义；`Protocol` 使用精确协议包版本把 UTF-8 Body 解码为
/// Document，并在 Document 实际变化时重新编码。未命中规则时不会重写 Body。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum HttpBodyProcessing {
    #[default]
    Plain,
    Protocol {
        package: ProtocolPackageRef,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct HttpListenerSettings {
    pub authentication: ForwardProxyAuthentication,
    pub mitm: MitmSettings,
    pub downstream_tls: DownstreamTlsSettings,
    pub request_body_codec: BodyCodecKind,
    pub response_body_codec: BodyCodecKind,
    pub body_processing: HttpBodyProcessing,
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
            body_processing: HttpBodyProcessing::Plain,
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
    #[serde(default)]
    pub tls_server_name: Option<String>,
    pub server_trust: Option<CertificateReferenceId>,
    pub client_identity: Option<CertificateReferenceId>,
}

impl Default for SocketUpstreamTlsSettings {
    fn default() -> Self {
        Self {
            verify_hostname: true,
            tls_server_name: None,
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

/// `LocalResponder` 只面向连接到 Listener 的 App，因此安全配置只能描述 App 侧传输。
// 该类型刻意不复用 SocketRelaySecurity：后者同时描述 App 与 Server 两侧，若复用就会让
// LocalResponder 可以携带实际不会执行的上游 TLS 配置。独立 tagged union 使 Wire 层也无法
// 表达上游地址、上游信任或客户端证书，从结构上避免“空地址表示无上游”一类哨兵值。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SocketDownstreamSecurity {
    #[default]
    Tcp,
    Tls {
        downstream_tls: SocketDownstreamTlsSettings,
    },
}

impl<'de> Deserialize<'de> for SocketDownstreamSecurity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Tcp {},
            Tls {
                downstream_tls: SocketDownstreamTlsSettings,
            },
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::Tcp {} => Self::Tcp,
            Wire::Tls { downstream_tls } => Self::Tls { downstream_tls },
        })
    }
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

/// Scripted 模式所需的完整、静态 Listener 配置。
/// 入口精确绑定一个 `package id + version`，不会自动选择、升级或回退协议包。选择协议包即表示
/// 两个方向固定执行 Frame、Decode、规则、Encode 与 Display，不再保存重复的阶段开关。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ScriptedSocketProcessing {
    /// 入口固定使用的协议包 ID 与精确 `SemVer`。
    pub package: ProtocolPackageRef,
}

/// Socket payload 的处理方式。
/// `Direct` 保持现有透明字节转发，不加载脚本、不切 Frame、不创建 Document；`Scripted` 按绑定
/// 协议包切分完整 Frame，并执行两个方向声明的完整处理链。
/// Wire 结构使用 `mode` + `settings`，例如 Direct 是 `{"mode":"direct"}`，Scripted 是
/// `{"mode":"scripted","settings":{...}}`。额外字段会被拒绝，防止 Direct 配置中夹带不会生效的
/// 脚本字段。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(
    tag = "mode",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SocketPayloadProcessing {
    #[default]
    Direct,
    Scripted(ScriptedSocketProcessing),
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
