use std::{fmt, net::SocketAddr, time::Duration};

use zeroize::Zeroizing;

use crate::{ErrorCode, ProxyError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketEndpoint {
    pub host: String,
    pub port: u16,
}

impl SocketEndpoint {
    pub fn validate(&self) -> Result<()> {
        let host = self.host.trim();
        if host.is_empty() || host != self.host {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "socket upstream host must be non-empty and trimmed",
            ));
        }
        if self.port == 0 {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "socket upstream port must be greater than zero",
            ));
        }
        if host.contains(['/', '?', '#', '@']) || host.contains("://") {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "socket upstream host must not contain a URL, path, query, or user info",
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct SocketTlsIdentity {
    pub certificate_chain_der: Vec<Vec<u8>>,
    pub private_key_pkcs8_der: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for SocketTlsIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketTlsIdentity")
            .field("certificate_count", &self.certificate_chain_der.len())
            .field("private_key_pkcs8_der", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct SocketDownstreamTlsConfig {
    pub server_identity: SocketTlsIdentity,
    pub client_trust_der: Vec<Vec<u8>>,
    pub client_authentication_required: bool,
}

#[derive(Clone, Debug)]
pub struct SocketUpstreamTlsConfig {
    pub server_trust_der: Vec<Vec<u8>>,
    pub client_identity: Option<SocketTlsIdentity>,
    pub verify_hostname: bool,
    pub tls_server_name: Option<String>,
}

#[derive(Clone, Debug)]
pub enum SocketRelaySecurity {
    Transparent,
    TcpToTls {
        upstream_tls: SocketUpstreamTlsConfig,
    },
    TlsToTcp {
        downstream_tls: SocketDownstreamTlsConfig,
    },
    TlsToTls {
        downstream_tls: SocketDownstreamTlsConfig,
        upstream_tls: SocketUpstreamTlsConfig,
    },
}

/// App 侧连接的传输安全配置。
///
/// `LocalResponder` 没有上游，因此不能复用同时描述上下游的
/// [`SocketRelaySecurity`]。独立类型可以从结构上保证本地应答模式不会意外携带、
/// 构造或调用上游 TLS 能力。
#[derive(Clone, Debug)]
pub enum SocketDownstreamSecurity {
    /// 接收普通 TCP 连接。
    Tcp,
    /// 在 App 侧终止 TLS，可选校验客户端证书。
    Tls {
        downstream_tls: SocketDownstreamTlsConfig,
    },
}

impl SocketRelaySecurity {
    pub fn terminates_downstream_tls(&self) -> bool {
        matches!(self, Self::TlsToTcp { .. } | Self::TlsToTls { .. })
    }

    pub fn originates_upstream_tls(&self) -> bool {
        matches!(self, Self::TcpToTls { .. } | Self::TlsToTls { .. })
    }
}

#[derive(Clone, Debug)]
pub struct SocketRelayConfig {
    pub bind_addr: SocketAddr,
    pub allowed_client_cidrs: Vec<String>,
    pub upstream: SocketEndpoint,
    pub security: SocketRelaySecurity,
    pub maximum_connections: u16,
    pub read_chunk_bytes: usize,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

/// 仅在 App 侧接收请求并写回应的 Socket Listener 配置。
///
/// 该结构故意没有 upstream、DNS、connect timeout 或上游 TLS 字段；这样即使调用方
/// 构造错误，也不能让 `LocalResponder` 偷偷建立上游连接。
#[derive(Clone, Debug)]
pub struct SocketLocalResponderConfig {
    pub bind_addr: SocketAddr,
    pub allowed_client_cidrs: Vec<String>,
    pub security: SocketDownstreamSecurity,
    pub maximum_connections: u16,
    pub read_chunk_bytes: usize,
    /// 下游 TLS 握手上限；纯 TCP 模式不会使用它。
    pub handshake_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketUpstreamConnectionTestResult {
    pub resolved_address: SocketAddr,
    pub transport: SocketUpstreamTransport,
    pub tls: Option<super::SocketTlsEvidence>,
    pub tls_server_name_candidates: Vec<String>,
    pub elapsed_millis: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketUpstreamTransport {
    Tcp,
    Tls,
}

impl SocketRelayConfig {
    pub fn validate(&self) -> Result<()> {
        self.upstream.validate()?;
        validate_connection_limit(self.maximum_connections)?;
        validate_read_chunk_bytes(self.read_chunk_bytes)?;
        validate_timeouts([
            ("connect", self.connect_timeout),
            ("read", self.read_timeout),
            ("write", self.write_timeout),
        ])
    }
}

impl SocketLocalResponderConfig {
    pub fn validate(&self) -> Result<()> {
        validate_connection_limit(self.maximum_connections)?;
        validate_read_chunk_bytes(self.read_chunk_bytes)?;
        validate_timeouts([
            ("handshake", self.handshake_timeout),
            ("read", self.read_timeout),
            ("write", self.write_timeout),
        ])
    }
}

fn validate_connection_limit(maximum_connections: u16) -> Result<()> {
    if !(1..=5_000).contains(&maximum_connections) {
        return Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "socket maximum connections must be between 1 and 5000",
        ));
    }
    Ok(())
}

fn validate_read_chunk_bytes(read_chunk_bytes: usize) -> Result<()> {
    if read_chunk_bytes == 0 {
        return Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "socket read chunk bytes must be greater than zero",
        ));
    }
    Ok(())
}

fn validate_timeouts<const N: usize>(timeouts: [(&str, Duration); N]) -> Result<()> {
    for (field, duration) in timeouts {
        if duration.is_zero() {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("socket {field} timeout must be greater than zero"),
            ));
        }
    }
    Ok(())
}
