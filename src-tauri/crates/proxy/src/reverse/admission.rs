//! 反向监听器的客户端网络准入与下游 TLS 接受。

use async_trait::async_trait;
use ring::digest::{SHA256, digest};
use tokio_rustls::TlsAcceptor;
use x509_parser::parse_x509_certificate;

use crate::transport::{
    AcceptedConnection, BoxIo, ConnectionAcceptor, ConnectionContext, TlsPeerIdentity,
};
use crate::{ErrorCode, ProxyError, Result};

#[derive(Clone)]
pub(super) struct ReverseConnectionAcceptor {
    pub(super) tls: Option<DownstreamTlsAcceptor>,
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
/// 它只负责 TLS/mTLS 握手与客户端证书证据；网络 CIDR 准入由各监听器自身处理。
#[derive(Clone)]
pub struct DownstreamTlsAcceptor {
    tls: TlsAcceptor,
}

impl std::fmt::Debug for DownstreamTlsAcceptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("DownstreamTlsAcceptor").finish()
    }
}

impl DownstreamTlsAcceptor {
    pub fn new(settings: &super::ReverseDownstreamTls) -> Result<Self> {
        Ok(Self {
            tls: super::build_server_acceptor(settings)?,
        })
    }

    pub async fn accept(
        &self,
        io: BoxIo,
        context: &ConnectionContext,
    ) -> Result<AcceptedConnection> {
        let stream = self.tls.accept(io).await.map_err(|error| {
            // 下游握手发生在 HTTP Session 创建之前。保留对端地址和 rustls 原始错误，
            // 让桌面诊断页能够区分 SNI、签名算法、协议版本和证书链等失败原因。
            tracing::warn!(
                peer = %context.peer_addr,
                error = %error,
                "reverse downstream TLS handshake failed"
            );
            ProxyError::new(
                ErrorCode::DownstreamTlsHandshakeFailed,
                format!(
                    "客户端到代理的 TLS 握手失败（对端 {}）：{error}",
                    context.peer_addr
                ),
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
