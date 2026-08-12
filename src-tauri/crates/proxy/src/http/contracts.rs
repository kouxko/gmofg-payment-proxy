use super::{
    Bytes, CancellationToken, ChannelId, Debug, FaultAction, InformationalResponseSink, Message,
    Method, ProxyError, Result, Uri, Uuid, async_trait,
};
use crate::transport::{ConnectionContext, HandshakePolicy, UpstreamSecurityEvidence};

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
