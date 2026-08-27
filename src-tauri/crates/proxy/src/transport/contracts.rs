use std::{
    fmt::Debug,
    net::SocketAddr,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use uuid::Uuid;

use crate::{Result, supervisor::ChannelId};

use super::TlsPeerIdentity;

pub const TLS_HANDSHAKE_POLICY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct ConnectionContext {
    pub runtime_epoch: Uuid,
    pub connection_id: Uuid,
    pub channel: ChannelId,
    pub peer_addr: SocketAddr,
    pub accepted_at: SystemTime,
    pub tls_peer: Option<TlsPeerIdentity>,
}

/// Synchronous, handshake-safe policy surface used from certificate verification.
#[async_trait]
pub trait HandshakePolicy: Debug + Send + Sync {
    async fn prepare_tls_handshake(&self, _context: &ConnectionContext) -> Result<()> {
        Ok(())
    }

    fn reject_tls_handshake(
        &self,
        _context: &ConnectionContext,
        _peer: &TlsPeerIdentity,
    ) -> Result<bool> {
        Ok(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamTransportSecurity {
    PlaintextHttp,
    Tls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamSecurityEvidence {
    pub resolved_address: SocketAddr,
    pub transport: UpstreamTransportSecurity,
    pub tls_version: Option<String>,
    pub cipher_suite: Option<String>,
    pub peer_subject: Option<String>,
    pub peer_sha256_fingerprint: Option<String>,
    pub hostname_verification_enabled: Option<bool>,
    pub client_identity_configured: bool,
    pub client_identity_submitted: bool,
}
