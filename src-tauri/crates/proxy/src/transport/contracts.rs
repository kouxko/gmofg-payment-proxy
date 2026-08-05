use super::{
    Bytes, CancellationToken, ChannelId, Debug, FaultAction, InformationalResponseSink, Message,
    Method, ProxyError, Result, SocketAddr, SystemTime, TlsPeerIdentity, Uri, Uuid, async_trait,
};

#[derive(Debug, Clone)]
pub struct ConnectionContext {
    pub runtime_epoch: Uuid,
    pub connection_id: Uuid,
    pub channel: ChannelId,
    pub peer_addr: SocketAddr,
    pub accepted_at: SystemTime,
    pub tls_peer: Option<TlsPeerIdentity>,
}

/// Synchronous, handshake-safe policy surface used from rustls certificate
/// verification. Implementations must not await or block on UI subscribers.
pub trait HandshakePolicy: Debug + Send + Sync {
    fn reject_tls_handshake(
        &self,
        _context: &ConnectionContext,
        _peer: &TlsPeerIdentity,
    ) -> Result<bool> {
        Ok(false)
    }
}

/// Application-facing hooks. Implementations must not block on UI subscribers.
#[async_trait]
pub trait PipelinePorts: HandshakePolicy {
    async fn runtime_stopping(&self, _epoch: Uuid) {}
    async fn connection_opened(&self, _context: &ConnectionContext) {}
    /// Reports the security properties of the concrete upstream connection used by this request.
    ///
    /// The callback runs immediately after TCP/TLS establishment, before request bytes are sent,
    /// so a later HTTP failure still leaves truthful transport evidence on the active session.
    async fn upstream_security_established(
        &self,
        _context: &ConnectionContext,
        _evidence: &UpstreamSecurityEvidence,
    ) {
    }
    async fn request(
        &self,
        _context: &ConnectionContext,
        _message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        Ok(Vec::new())
    }
    async fn response(
        &self,
        _context: &ConnectionContext,
        _message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        Ok(Vec::new())
    }
    async fn connection_closed(&self, _context: &ConnectionContext, _result: &Result<()>) {}
    async fn runtime_fault(&self, _epoch: Uuid, _channel: ChannelId, _error: &ProxyError) {}
}

#[derive(Debug, Default)]
pub struct NoopPipelinePorts;
impl HandshakePolicy for NoopPipelinePorts {}
impl PipelinePorts for NoopPipelinePorts {}

#[derive(Debug, Clone)]
pub struct ForwardRequest {
    pub method: Method,
    pub uri: Uri,
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamTransportSecurity {
    PlaintextHttp,
    Tls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Public metadata from the exact upstream socket used by one HTTP exchange.
/// Certificate bytes and private identity material never cross this boundary.
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

#[derive(Debug, Clone)]
pub struct UpstreamExchange {
    pub informational_heads: Vec<Bytes>,
    pub final_response: Message,
}

impl From<Message> for UpstreamExchange {
    fn from(final_response: Message) -> Self {
        Self {
            informational_heads: Vec::new(),
            final_response,
        }
    }
}

#[async_trait]
pub trait UpstreamConnector: Debug + Send + Sync {
    async fn send(
        &self,
        context: &ConnectionContext,
        ports: &dyn PipelinePorts,
        request: ForwardRequest,
        actions: &[FaultAction],
        informational: Option<&InformationalResponseSink>,
        cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange>;
}
