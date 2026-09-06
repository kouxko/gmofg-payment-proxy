use super::{
    Bytes, CancellationToken, ChannelId, Debug, FaultAction, InformationalResponseSink, Message,
    Method, ProxyError, Result, Uri, Uuid, async_trait,
};
use crate::http::UpstreamWriteReceipt;
use crate::transport::{ConnectionContext, HandshakePolicy, UpstreamSecurityEvidence};
use crate::{HttpJointEvaluation, SocketJointEvaluation, SocketPayloadDirection};
use intercept_proxy_exchange::SocketContext;

/// Application-facing HTTP wire policy and lifecycle hooks.
///
/// `apply_*_policy` runs once during Exchange write preparation against the authoritative framed
/// `Message`. The joint variants receive the protocol-specific Document transaction explicitly so
/// the application rule actor can gate, encode and commit lifecycle state as one operation. Ports
/// must not retain that transaction or block on UI subscribers.
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
    async fn apply_request_joint_policy(
        &self,
        _context: &ConnectionContext,
        _request: &HttpRequestMetadata,
        _message: &mut Message,
        _evaluation: Box<dyn HttpJointEvaluation>,
    ) -> Result<Vec<FaultAction>> {
        Err(ProxyError::new(
            crate::ErrorCode::ConfigInvalid,
            "HTTP Document transaction requires a joint pipeline implementation",
        ))
    }
    async fn apply_response_joint_policy(
        &self,
        _context: &ConnectionContext,
        _request: &HttpRequestMetadata,
        _message: &mut Message,
        _evaluation: Box<dyn HttpJointEvaluation>,
    ) -> Result<Vec<FaultAction>> {
        Err(ProxyError::new(
            crate::ErrorCode::ConfigInvalid,
            "HTTP Document transaction requires a joint pipeline implementation",
        ))
    }
    async fn apply_socket_policy(
        &self,
        _context: &ConnectionContext,
        direction: SocketPayloadDirection,
        evaluation: Box<dyn SocketJointEvaluation>,
    ) -> Result<SocketContext> {
        evaluation.encode().await.map_err(|error| {
            let exchange_direction = match direction {
                SocketPayloadDirection::AppToUpstream | SocketPayloadDirection::LocalExchange => {
                    intercept_proxy_exchange::DirectionKind::Upstream
                }
                SocketPayloadDirection::UpstreamToApp => {
                    intercept_proxy_exchange::DirectionKind::Downstream
                }
            };
            let message = error.message.clone();
            ProxyError::from_exchange_error(
                crate::ErrorCode::ExternalPackageCallFailed,
                message,
                error.at(
                    intercept_proxy_exchange::ErrorStage::Encode,
                    exchange_direction,
                ),
            )
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
    #[allow(clippy::too_many_arguments)]
    async fn send(
        &self,
        context: &ConnectionContext,
        ports: &dyn PipelinePorts,
        request: ForwardRequest,
        actions: &[FaultAction],
        write_receipt: UpstreamWriteReceipt,
        informational: Option<&InformationalResponseSink>,
        cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use intercept_proxy_exchange::{DirectionKind, Error, ErrorStage};
    use std::time::SystemTime;

    struct FailingSocketEncode;

    #[async_trait]
    impl SocketJointEvaluation for FailingSocketEncode {
        fn gate(&mut self, _rule_id: Uuid) -> Result<crate::JointRuleConditionEvaluation> {
            unreachable!("default policy test only exercises Encode")
        }

        async fn encode(self: Box<Self>) -> std::result::Result<SocketContext, Error> {
            Err(
                Error::coded("ENCODE_FAILED", "Rules: misleading stage text")
                    .with_external_package_call(
                        intercept_proxy_exchange::ExternalPackageCallFailure {
                            package: intercept_proxy_exchange::ProtocolPackageRef {
                                id: intercept_proxy_exchange::ProtocolPackageId::new(
                                    "default-socket-policy",
                                )
                                .unwrap(),
                                version: intercept_proxy_exchange::ProtocolPackageVersion::new(
                                    "1.0.0",
                                )
                                .unwrap(),
                            },
                            direction: intercept_proxy_exchange::ProtocolDirection::Downstream,
                            stage: intercept_proxy_exchange::ExternalPackageCallStage::Encode,
                            method: "hooks.encode".into(),
                            request_id: None,
                            remote_code: None,
                            stable_code: Some("ENCODE_FAILED".into()),
                            remote_message: None,
                            remote_data_summary: None,
                        },
                    ),
            )
        }
    }

    #[tokio::test]
    async fn default_socket_policy_preserves_encode_error_location() {
        let context = ConnectionContext {
            runtime_epoch: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
            channel: ChannelId::new("listener").unwrap(),
            peer_addr: "127.0.0.1:12345".parse().unwrap(),
            accepted_at: SystemTime::now(),
            tls_peer: None,
        };
        let error = NoopPipelinePorts
            .apply_socket_policy(
                &context,
                SocketPayloadDirection::UpstreamToApp,
                Box::new(FailingSocketEncode),
            )
            .await
            .unwrap_err();

        assert_eq!(
            error.code,
            crate::ErrorCode::ExternalPackageCallFailed.as_str()
        );
        assert_eq!(error.exchange_code.as_deref(), Some("ENCODE_FAILED"));
        assert_eq!(error.stage, Some(ErrorStage::Encode));
        assert_eq!(error.direction, Some(DirectionKind::Downstream));
        assert!(error.external_package_call.is_some());
    }
}
