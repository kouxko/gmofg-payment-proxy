//! Socket Listener 的显式网络拓扑。

use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    DEFAULT_SOCKET_MAXIMUM_CONNECTIONS, SocketDownstreamSecurity, SocketEndpoint,
    SocketPayloadProcessing, SocketRelaySecurity,
};

pub const DEFAULT_SOCKET_READ_CHUNK_BYTES: u32 = 16 * 1024;
pub const DEFAULT_SOCKET_DIAGNOSTIC_EVENT_CAPACITY: u32 = 256;
pub const DEFAULT_SOCKET_DIAGNOSTIC_MEMORY_BYTES: u64 = 1024 * 1024;

/// Socket 运行时所有内存队列和单次 OS read 的显式资源合同。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuntimeLimits {
    pub read_chunk_bytes: u32,
    pub diagnostic_event_capacity: u32,
    pub diagnostic_memory_bytes: u64,
}

impl Default for SocketRuntimeLimits {
    fn default() -> Self {
        Self {
            read_chunk_bytes: DEFAULT_SOCKET_READ_CHUNK_BYTES,
            diagnostic_event_capacity: DEFAULT_SOCKET_DIAGNOSTIC_EVENT_CAPACITY,
            diagnostic_memory_bytes: DEFAULT_SOCKET_DIAGNOSTIC_MEMORY_BYTES,
        }
    }
}

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRelaySettings {
    /// 网络拓扑。Relay 才拥有 Server 上游；LocalResponder 只拥有 App 侧安全设置。
    pub topology: SocketTopology,
    /// Listener 同时接受的最大 Socket 连接数。
    pub maximum_connections: u16,
    /// 经过 Workspace 校验后原样注入 Proxy/diagnostic runtime；运行时不得静默修正。
    pub runtime_limits: SocketRuntimeLimits,
    /// Frame/payload 处理方式。
    pub processing: SocketPayloadProcessing,
}

impl Default for SocketRelaySettings {
    fn default() -> Self {
        Self {
            topology: SocketTopology::default(),
            maximum_connections: DEFAULT_SOCKET_MAXIMUM_CONNECTIONS,
            runtime_limits: SocketRuntimeLimits::default(),
            processing: SocketPayloadProcessing::Direct,
        }
    }
}

impl SocketRelaySettings {
    /// 构造保持现有透明/脚本 Relay 语义的显式拓扑配置。
    #[must_use]
    pub fn relay(
        upstream: SocketEndpoint,
        security: SocketRelaySecurity,
        maximum_connections: u16,
        processing: SocketPayloadProcessing,
    ) -> Self {
        Self {
            topology: SocketTopology::Relay(SocketRelayTopology { upstream, security }),
            maximum_connections,
            runtime_limits: SocketRuntimeLimits::default(),
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
