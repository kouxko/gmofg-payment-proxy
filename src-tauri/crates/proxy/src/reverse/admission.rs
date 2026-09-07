//! 反向监听器的客户端网络准入与下游 TLS 接受。

use async_trait::async_trait;
use ring::digest::{SHA256, digest};
use std::sync::Arc;

use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use x509_parser::parse_x509_certificate;

use crate::transport::{
    AcceptedConnection, BoxIo, ConnectionAcceptor, ConnectionContext, TLS_HANDSHAKE_POLICY_TIMEOUT,
    TlsPeerIdentity,
};
use crate::{ErrorCode, ProxyError, Result};

#[derive(Clone)]
pub(crate) struct ReverseConnectionAcceptor {
    pub(crate) tls: Option<DownstreamTlsAcceptor>,
}

impl std::fmt::Debug for ReverseConnectionAcceptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReverseConnectionAcceptor")
            .field("tls", &self.tls.is_some())
            .finish()
    }
}

#[async_trait]
impl ConnectionAcceptor for ReverseConnectionAcceptor {
    async fn accept(&self, io: BoxIo, context: &ConnectionContext) -> Result<AcceptedConnection> {
        let Some(acceptor) = &self.tls else {
            return Ok(AcceptedConnection { io, tls_peer: None });
        };
        acceptor.accept(io, context).await
    }
}

/// 可复用于固定转发和动态正向代理的下游 TLS 接受器。
///
/// 它只负责 TLS/mTLS 握手与客户端证书证据；客户端网络不设额外准入策略。
#[derive(Clone)]
pub struct DownstreamTlsAcceptor {
    tls: TlsAcceptor,
    handshake_capacity: Arc<tokio::sync::Semaphore>,
}

const MAX_BLOCKING_REVERSE_HANDSHAKES: usize = 16;

impl std::fmt::Debug for DownstreamTlsAcceptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("DownstreamTlsAcceptor").finish()
    }
}

impl DownstreamTlsAcceptor {
    pub fn new(settings: &super::ReverseDownstreamTls) -> Result<Self> {
        Ok(Self {
            tls: super::build_server_acceptor(settings)?,
            handshake_capacity: Arc::new(tokio::sync::Semaphore::new(
                MAX_BLOCKING_REVERSE_HANDSHAKES,
            )),
        })
    }

    pub async fn accept(
        &self,
        io: BoxIo,
        context: &ConnectionContext,
    ) -> Result<AcceptedConnection> {
        let permit = Arc::clone(&self.handshake_capacity)
            .acquire_owned()
            .await
            .map_err(|_| reverse_handshake_error("capacity closed"))?;
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|error| reverse_handshake_error(&format!("runtime unavailable: {error}")))?;
        let tls = self.tls.clone();
        let peer = context.peer_addr;
        let cancellation = CancellationToken::new();
        let mut cancellation_guard = ReverseHandshakeCancellation::new(cancellation.clone());
        let joined = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            runtime.block_on(async move {
                tokio::time::timeout(TLS_HANDSHAKE_POLICY_TIMEOUT, async move {
                    tokio::select! {
                        biased;
                        () = cancellation.cancelled() => Err(reverse_handshake_error("cancelled")),
                        result = tls.accept(io) => accepted_connection(result, peer),
                    }
                })
                .await
                .map_err(|_| reverse_handshake_error("timed out"))?
            })
        })
        .await;
        cancellation_guard.disarm();
        joined.map_err(|error| reverse_handshake_error(&format!("task failed: {error}")))?
    }
}

fn accepted_connection(
    result: std::io::Result<tokio_rustls::server::TlsStream<BoxIo>>,
    peer: std::net::SocketAddr,
) -> Result<AcceptedConnection> {
    let stream = result.map_err(|error| {
        tracing::warn!(peer = %peer, error = %error, "reverse downstream TLS handshake failed");
        ProxyError::new(
            ErrorCode::DownstreamTlsHandshakeFailed,
            format!("客户端到代理的 TLS 握手失败（对端 {peer}）：{error}"),
        )
    })?;
    let tls_peer = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .map(|certificate| peer_identity(certificate.as_ref()))
        .transpose()?;
    Ok(AcceptedConnection {
        io: Box::new(stream),
        tls_peer,
    })
}

fn reverse_handshake_error(message: &str) -> ProxyError {
    ProxyError::new(
        ErrorCode::DownstreamTlsHandshakeFailed,
        format!("reverse downstream TLS handshake {message}"),
    )
}

struct ReverseHandshakeCancellation {
    cancellation: CancellationToken,
    armed: bool,
}

impl ReverseHandshakeCancellation {
    fn new(cancellation: CancellationToken) -> Self {
        Self {
            cancellation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ReverseHandshakeCancellation {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel();
        }
    }
}

fn peer_identity(certificate_der: &[u8]) -> Result<TlsPeerIdentity> {
    let (_, certificate) = parse_x509_certificate(certificate_der).map_err(super::config_error)?;
    let fingerprint = digest(&SHA256, certificate_der)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    Ok(TlsPeerIdentity {
        sha256_fingerprint: fingerprint,
        subject_summary: certificate.subject().to_string(),
    })
}
