use std::{fmt, sync::Arc};

use async_trait::async_trait;
use ring::digest::{SHA256, digest};
use rustls::{
    DigitallySignedStruct, DistinguishedName, Error, RootCertStore, ServerConfig, SignatureScheme,
    client::danger::HandshakeSignatureValid,
    pki_types::{CertificateDer, UnixTime},
    server::{
        WebPkiClientVerifier,
        danger::{ClientCertVerified, ClientCertVerifier},
    },
    sign::{CertifiedKey, SingleCertAndKey},
    version::TLS12,
};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

use super::support::{
    application_verification_failure, certified_key, peer_identity, tls_config, tls_handshake,
};
use crate::transport::{
    AcceptedConnection, BoxIo, ConnectionAcceptor, ConnectionContext, HandshakePolicy,
    TLS_HANDSHAKE_POLICY_TIMEOUT,
};
use crate::{ErrorCode, ProxyError, Result};

#[derive(Clone)]
pub struct ServerTlsAdapter {
    certified_key: Arc<CertifiedKey>,
    client_ca_der: Arc<Vec<u8>>,
    allowed_client_fingerprint: Option<Vec<u8>>,
    handshake_policy: Arc<dyn HandshakePolicy>,
    handshake_capacity: Arc<tokio::sync::Semaphore>,
}

const MAX_BLOCKING_HANDSHAKES: usize = 16;

impl fmt::Debug for ServerTlsAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerTlsAdapter")
            .field(
                "has_fingerprint_pin",
                &self.allowed_client_fingerprint.is_some(),
            )
            .field("private_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl ServerTlsAdapter {
    /// 构建下游 mTLS 服务端的不可变证书快照。
    ///
    /// CA、服务端证书或私钥任一无效都会在监听发布前失败；运行中不会热换其中一部分，
    /// 因此一次握手看到的链和私钥始终来自同一 epoch。
    pub fn build(
        certificate_chain: Vec<Vec<u8>>,
        private_key_pkcs8_der: Vec<u8>,
        client_ca_der: Vec<u8>,
        allowed_client_fingerprint: Option<Vec<u8>>,
        handshake_policy: Arc<dyn HandshakePolicy>,
    ) -> Result<Self> {
        if certificate_chain.is_empty() {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "server certificate chain is empty",
            ));
        }
        let certified_key = certified_key(certificate_chain, private_key_pkcs8_der)?;
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(client_ca_der.clone()))
            .map_err(tls_config)?;
        WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            Arc::new(rustls::crypto::ring::default_provider()),
        )
        .build()
        .map_err(tls_config)?;
        Ok(Self {
            certified_key: Arc::new(certified_key),
            client_ca_der: Arc::new(client_ca_der),
            allowed_client_fingerprint,
            handshake_policy,
            handshake_capacity: Arc::new(tokio::sync::Semaphore::new(MAX_BLOCKING_HANDSHAKES)),
        })
    }

    fn acceptor_for(&self, context: &ConnectionContext) -> Result<TlsAcceptor> {
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(self.client_ca_der.as_ref().clone()))
            .map_err(tls_config)?;
        let webpki = WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots),
            Arc::new(rustls::crypto::ring::default_provider()),
        )
        .build()
        .map_err(tls_config)?;
        let verifier = PolicyClientCertVerifier {
            webpki,
            allowed_fingerprint: self.allowed_client_fingerprint.clone(),
            policy: Arc::clone(&self.handshake_policy),
            context: context.clone(),
        };
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&TLS12])
                .map_err(tls_config)?
                .with_client_cert_verifier(Arc::new(verifier))
                .with_cert_resolver(Arc::new(SingleCertAndKey::from(self.certified_key.clone())));
        Ok(TlsAcceptor::from(Arc::new(config)))
    }
}

#[async_trait]
impl ConnectionAcceptor for ServerTlsAdapter {
    async fn accept(&self, io: BoxIo, context: &ConnectionContext) -> Result<AcceptedConnection> {
        self.handshake_policy.prepare_tls_handshake(context).await?;
        let acceptor = self.acceptor_for(context)?;
        let permit = Arc::clone(&self.handshake_capacity)
            .acquire_owned()
            .await
            .map_err(|_| {
                ProxyError::new(
                    ErrorCode::TlsHandshakeFailed,
                    "TLS handshake capacity closed",
                )
            })?;
        let runtime = tokio::runtime::Handle::try_current().map_err(|error| {
            ProxyError::new(
                ErrorCode::TlsHandshakeFailed,
                format!("TLS handshake runtime unavailable: {error}"),
            )
        })?;
        let cancellation = CancellationToken::new();
        let mut cancellation_guard = HandshakeCancellation::new(cancellation.clone());
        let joined = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            runtime.block_on(async move {
                tokio::time::timeout(TLS_HANDSHAKE_POLICY_TIMEOUT, async move {
                    tokio::select! {
                        biased;
                        () = cancellation.cancelled() => Err(ProxyError::new(
                            ErrorCode::TlsHandshakeFailed,
                            "TLS handshake cancelled",
                        )),
                        result = acceptor.accept(io) => {
                            let stream = result.map_err(tls_handshake)?;
                            accepted_tls_connection(stream)
                        }
                    }
                })
                .await
                .map_err(|_| {
                    ProxyError::new(ErrorCode::TlsHandshakeFailed, "TLS handshake timed out")
                })?
            })
        })
        .await;
        cancellation_guard.disarm();
        joined.map_err(|error| {
            ProxyError::new(
                ErrorCode::TlsHandshakeFailed,
                format!("TLS handshake task failed: {error}"),
            )
        })?
    }
}

fn accepted_tls_connection(
    stream: tokio_rustls::server::TlsStream<BoxIo>,
) -> Result<AcceptedConnection> {
    let certificate = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| {
            ProxyError::new(ErrorCode::TlsHandshakeFailed, "client certificate missing")
        })?;
    Ok(AcceptedConnection {
        tls_peer: Some(peer_identity(certificate.as_ref())?),
        io: Box::new(stream),
    })
}

struct HandshakeCancellation {
    cancellation: CancellationToken,
    armed: bool,
}

impl HandshakeCancellation {
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

impl Drop for HandshakeCancellation {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel();
        }
    }
}

#[derive(Debug)]
struct PolicyClientCertVerifier {
    webpki: Arc<dyn ClientCertVerifier>,
    allowed_fingerprint: Option<Vec<u8>>,
    policy: Arc<dyn HandshakePolicy>,
    context: ConnectionContext,
}

impl ClientCertVerifier for PolicyClientCertVerifier {
    fn offer_client_auth(&self) -> bool {
        self.webpki.offer_client_auth()
    }

    fn client_auth_mandatory(&self) -> bool {
        self.webpki.client_auth_mandatory()
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.webpki.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, Error> {
        let verified = self
            .webpki
            .verify_client_cert(end_entity, intermediates, now)?;
        let actual = digest(&SHA256, end_entity.as_ref());
        if self
            .allowed_fingerprint
            .as_ref()
            .is_some_and(|expected| actual.as_ref() != expected)
        {
            return Err(application_verification_failure());
        }
        let identity =
            peer_identity(end_entity.as_ref()).map_err(|_| application_verification_failure())?;
        if self
            .policy
            .reject_tls_handshake(&self.context, &identity)
            .unwrap_or(true)
        {
            return Err(application_verification_failure());
        }
        Ok(verified)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, Error> {
        self.webpki.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, Error> {
        self.webpki.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.webpki.supported_verify_schemes()
    }
}
