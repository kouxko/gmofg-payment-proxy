//! Socket-specific Listener wire model.

use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

use crate::{CertificateReferenceId, ProtocolPackageRef};

use super::DownstreamClientAuthentication;

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
