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
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketUpstreamConnectionTestResult {
    pub resolved_address: SocketAddr,
    pub transport: SocketUpstreamTransport,
    pub tls: Option<super::SocketTlsEvidence>,
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
        if !(1..=5_000).contains(&self.maximum_connections) {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "socket maximum connections must be between 1 and 5000",
            ));
        }
        for (field, duration) in [
            ("connect", self.connect_timeout),
            ("read", self.read_timeout),
            ("write", self.write_timeout),
        ] {
            if duration.is_zero() {
                return Err(ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    format!("socket {field} timeout must be greater than zero"),
                ));
            }
        }
        Ok(())
    }
}
