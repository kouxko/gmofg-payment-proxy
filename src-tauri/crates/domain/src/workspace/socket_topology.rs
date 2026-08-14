//! Socket Listener 的显式网络拓扑与历史 Wire 迁移。

use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

use super::{
    DEFAULT_SOCKET_MAXIMUM_CONNECTIONS, SocketDownstreamSecurity, SocketEndpoint,
    SocketPayloadProcessing, SocketRelaySecurity,
};

/// 具有真实 Server 上游的 Socket 转发拓扑。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRelayTopology {
    pub upstream: SocketEndpoint,
    pub security: SocketRelaySecurity,
}

/// 不连接 Server、而是由协议包在本机生成响应的 Socket 拓扑。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketLocalResponderTopology {
    pub downstream_security: SocketDownstreamSecurity,
}

/// Socket Listener 的网络拓扑。变体自身拥有且只拥有该模式可用的字段。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(
    tag = "mode",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SocketTopology {
    Relay(SocketRelayTopology),
    LocalResponder(SocketLocalResponderTopology),
}

impl Default for SocketTopology {
    fn default() -> Self {
        Self::Relay(SocketRelayTopology {
            upstream: SocketEndpoint::default(),
            security: SocketRelaySecurity::Transparent,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
pub struct SocketRelaySettings {
    /// 网络拓扑。Relay 才拥有 Server 上游；LocalResponder 只拥有 App 侧安全设置。
    pub topology: SocketTopology,
    /// Listener 同时接受的最大 Socket 连接数。
    pub maximum_connections: u16,
    /// Frame/payload 处理方式。历史配置没有该字段时必须保持透明直通。
    #[serde(default)]
    pub processing: SocketPayloadProcessing,
}

impl Default for SocketRelaySettings {
    fn default() -> Self {
        Self {
            topology: SocketTopology::default(),
            maximum_connections: DEFAULT_SOCKET_MAXIMUM_CONNECTIONS,
            processing: SocketPayloadProcessing::Direct,
        }
    }
}

impl SocketRelaySettings {
    /// 构造保持现有透明/脚本 Relay 语义的显式拓扑配置。
    #[must_use]
    pub const fn relay(
        upstream: SocketEndpoint,
        security: SocketRelaySecurity,
        maximum_connections: u16,
        processing: SocketPayloadProcessing,
    ) -> Self {
        Self {
            topology: SocketTopology::Relay(SocketRelayTopology { upstream, security }),
            maximum_connections,
            processing,
        }
    }

    #[must_use]
    pub const fn relay_topology(&self) -> Option<&SocketRelayTopology> {
        match &self.topology {
            SocketTopology::Relay(relay) => Some(relay),
            SocketTopology::LocalResponder(_) => None,
        }
    }
}

impl<'de> Deserialize<'de> for SocketRelaySettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        /// 当前格式。严格拒绝跨拓扑字段，避免 `LocalResponder` 悄悄吞掉伪造的 upstream/TLS。
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Current {
            topology: SocketTopology,
            maximum_connections: u16,
            #[serde(default)]
            processing: SocketPayloadProcessing,
        }

        /// T18 之前的格式。它总是具有真实 upstream，因此只能迁移为 Relay，绝不能猜成
        /// LocalResponder。缺少 processing 的更早配置继续按原义迁移为 Direct。
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Legacy {
            upstream: SocketEndpoint,
            security: SocketRelaySecurity,
            maximum_connections: u16,
            #[serde(default)]
            processing: SocketPayloadProcessing,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Current(Current),
            Legacy(Legacy),
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::Current(value) => Self {
                topology: value.topology,
                maximum_connections: value.maximum_connections,
                processing: value.processing,
            },
            Wire::Legacy(value) => Self {
                topology: SocketTopology::Relay(SocketRelayTopology {
                    upstream: value.upstream,
                    security: value.security,
                }),
                maximum_connections: value.maximum_connections,
                processing: value.processing,
            },
        })
    }
}
