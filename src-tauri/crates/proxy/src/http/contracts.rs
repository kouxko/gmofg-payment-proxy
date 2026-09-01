use super::{
    Bytes, CancellationToken, ChannelId, Debug, FaultAction, InformationalResponseSink, Message,
    Method, ProxyError, Result, Uri, Uuid, async_trait,
};
use crate::transport::{ConnectionContext, HandshakePolicy, UpstreamSecurityEvidence};
use crate::{SocketJointEvaluation, SocketPayloadDirection};
use intercept_proxy_exchange::SocketContext;

/// Application-facing HTTP wire policy and lifecycle hooks.
///
/// `apply_*_policy` runs once on the framed HTTP `Message` before that direction enters Exchange
/// Decode. It may implement product HTTP mutation, session, capture and fault-action
/// semantics, but it must never invoke a protocol package or an Exchange capability. Consequently
/// Decode/Display/Rules/Encode remain the only protocol stages and cannot be hidden behind this
/// port. Implementations must not block on UI subscribers.
#[async_trait]
pub trait PipelinePorts: HandshakePolicy {
    async fn runtime_started(&self, _epoch: Uuid) {}
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
    async fn apply_request_policy(
        &self,
        _context: &ConnectionContext,
        _request: &HttpRequestMetadata,
        _message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        Ok(Vec::new())
    }
    async fn apply_response_policy(
        &self,
        _context: &ConnectionContext,
        _request: &HttpRequestMetadata,
        _message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        Ok(Vec::new())
    }
    async fn apply_socket_policy(
        &self,
        _context: &ConnectionContext,
        _direction: SocketPayloadDirection,
        evaluation: Box<dyn SocketJointEvaluation>,
    ) -> Result<SocketContext> {
        evaluation.encode().await.map_err(|error| {
            let mut mapped =
                ProxyError::new(crate::ErrorCode::ExternalPackageCallFailed, error.message);
            mapped.external_package_call = error.external_package_call;
            mapped
        })
    }
    async fn connection_closed(&self, _context: &ConnectionContext, _result: &Result<()>) {}
    async fn runtime_fault(&self, _epoch: Uuid, _channel: ChannelId, _error: &ProxyError) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequestMetadata {
    pub method: String,
    pub request_target: String,
}

impl HttpRequestMetadata {
    pub fn from_method_and_uri(method: &Method, uri: &Uri) -> Result<Self> {
        let request_target = uri.path_and_query().ok_or_else(|| {
            ProxyError::new(
                crate::ErrorCode::ConfigInvalid,
                "HTTP request URI has no path-and-query target",
            )
        })?;
        Ok(Self {
            method: method.as_str().to_owned(),
            request_target: request_target.as_str().to_owned(),
        })
    }
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
