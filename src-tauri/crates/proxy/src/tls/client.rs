use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use rustls::{
    ClientConfig, RootCertStore, SignatureScheme,
    client::ResolvesClientCert,
    pki_types::{CertificateDer, ServerName},
    sign::SingleCertAndKey,
    version::TLS12,
};
use tokio_rustls::TlsConnector;

use super::support::{certified_key, peer_identity, tls_config, tls_handshake};
use crate::transport::{BoxIo, TlsPeerIdentity};
use crate::{ErrorCode, ProxyError, Result};

#[derive(Clone)]
pub struct ClientTlsAdapter {
    config: Arc<ClientConfig>,
    hostname_verification_enabled: bool,
    client_identity_configured: bool,
}

pub struct ClientTlsConnection {
    pub io: BoxIo,
    pub evidence: ClientTlsHandshakeEvidence,
}

impl fmt::Debug for ClientTlsConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientTlsConnection")
            .field("io", &"<TLS stream>")
            .field("evidence", &self.evidence)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientTlsHandshakeEvidence {
    pub tls_version: String,
    pub cipher_suite: String,
    pub peer: TlsPeerIdentity,
    pub hostname_verification_enabled: bool,
    pub client_identity_configured: bool,
    pub client_identity_submitted: bool,
}

impl fmt::Debug for ClientTlsAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientTlsAdapter")
            .field(
                "hostname_verification_enabled",
                &self.hostname_verification_enabled,
            )
            .field(
                "client_identity_configured",
                &self.client_identity_configured,
            )
            .finish_non_exhaustive()
    }
}

impl ClientTlsAdapter {
    /// 复用已经按动态 Workspace 配置构建完成的 rustls connector。
    ///
    /// 该入口仅供同一 runtime crate 内的通用反向监听器使用；证书链、主机名策略与可选
    /// 客户端身份仍由其构建阶段一次性校验，HTTP 管线不接触原始证书配置。
    pub(crate) fn from_config(
        config: ClientConfig,
        hostname_verification_enabled: bool,
        client_identity_configured: bool,
    ) -> Self {
        Self {
            config: Arc::new(config),
            hostname_verification_enabled,
            client_identity_configured,
        }
    }

    /// 构建访问上游的 mTLS 客户端快照；仅信任显式提供的 CA，并携带指定客户端身份。
    pub fn build(
        certificate_chain: Vec<Vec<u8>>,
        private_key_pkcs8_der: Vec<u8>,
        upstream_ca_der: Vec<u8>,
    ) -> Result<Self> {
        if certificate_chain.is_empty() {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "upstream client certificate chain is empty",
            ));
        }
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(upstream_ca_der))
            .map_err(tls_config)?;
        let certified_key = certified_key(certificate_chain, private_key_pkcs8_der)?;
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&TLS12])
                .map_err(tls_config)?
                .with_root_certificates(roots)
                .with_client_cert_resolver(Arc::new(SingleCertAndKey::from(certified_key)));
        Ok(Self::from_config(config, true, true))
    }

    /// 拥有所有权的 `ServerName` 同时用于真实 SNI 与 `WebPKI` 主机名/IP 校验。
    ///
    /// TCP 已连接不代表这里成功；链、主机名、协议版本或客户端身份任一不符都会终止 TLS。
    pub async fn connect(&self, domain: &str, io: BoxIo) -> Result<BoxIo> {
        Ok(self.connect_with_evidence(domain, io).await?.io)
    }

    /// Completes the real upstream handshake and returns only public evidence from that socket.
    /// A per-connection resolver records whether rustls actually selected/submitted a configured
    /// client identity, rather than confusing "configured" with "requested by the server".
    pub async fn connect_with_evidence(
        &self,
        domain: &str,
        io: BoxIo,
    ) -> Result<ClientTlsConnection> {
        let server_name = ServerName::try_from(domain.to_owned()).map_err(tls_config)?;
        let submitted = Arc::new(AtomicBool::new(false));
        let mut config = self.config.as_ref().clone();
        if self.client_identity_configured {
            config.client_auth_cert_resolver = Arc::new(TrackingClientCertResolver {
                inner: Arc::clone(&config.client_auth_cert_resolver),
                submitted: Arc::clone(&submitted),
            });
        }
        let stream = TlsConnector::from(Arc::new(config))
            .connect(server_name, io)
            .await
            .map_err(tls_handshake)?;
        let connection = stream.get_ref().1;
        let certificate = connection
            .peer_certificates()
            .and_then(|certificates| certificates.first())
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCode::CertificateInvalid,
                    "upstream TLS handshake returned no peer certificate",
                )
            })?;
        let evidence = ClientTlsHandshakeEvidence {
            tls_version: connection.protocol_version().map_or_else(
                || "未知".to_owned(),
                |version| match version {
                    rustls::ProtocolVersion::TLSv1_2 => "TLS 1.2".to_owned(),
                    other => format!("{other:?}"),
                },
            ),
            cipher_suite: connection
                .negotiated_cipher_suite()
                .map_or_else(|| "未知".to_owned(), |suite| format!("{:?}", suite.suite())),
            peer: peer_identity(certificate.as_ref())?,
            hostname_verification_enabled: self.hostname_verification_enabled,
            client_identity_configured: self.client_identity_configured,
            client_identity_submitted: submitted.load(Ordering::Acquire),
        };
        Ok(ClientTlsConnection {
            io: Box::new(stream),
            evidence,
        })
    }
}

#[derive(Debug)]
struct TrackingClientCertResolver {
    inner: Arc<dyn ResolvesClientCert>,
    submitted: Arc<AtomicBool>,
}

impl ResolvesClientCert for TrackingClientCertResolver {
    fn resolve(
        &self,
        root_hint_subjects: &[&[u8]],
        sigschemes: &[SignatureScheme],
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        let resolved = self.inner.resolve(root_hint_subjects, sigschemes);
        if resolved.is_some() {
            self.submitted.store(true, Ordering::Release);
        }
        resolved
    }

    fn only_raw_public_keys(&self) -> bool {
        self.inner.only_raw_public_keys()
    }

    fn has_certs(&self) -> bool {
        self.inner.has_certs()
    }
}
