//! rustls TLS 1.2-only mTLS adapters (`PROXY-001` through `PROXY-005`).

use std::{fmt, sync::Arc};

use async_trait::async_trait;
use ring::digest::{SHA256, digest};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, DistinguishedName, Error, RootCertStore,
    ServerConfig, SignatureScheme,
    client::danger::HandshakeSignatureValid,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::{
        WebPkiClientVerifier,
        danger::{ClientCertVerified, ClientCertVerifier},
    },
    sign::{CertifiedKey, SingleCertAndKey},
    version::TLS12,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use x509_parser::parse_x509_certificate;
use zeroize::Zeroize;

use crate::transport::{
    AcceptedConnection, BoxIo, ConnectionAcceptor, ConnectionContext, HandshakePolicy,
    TlsPeerIdentity,
};
use crate::{ErrorCode, ProxyError, Result};

#[derive(Clone)]
pub struct ServerTlsAdapter {
    certified_key: Arc<CertifiedKey>,
    client_ca_der: Arc<Vec<u8>>,
    allowed_client_fingerprint: Option<Vec<u8>>,
    handshake_policy: Arc<dyn HandshakePolicy>,
}

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
        let stream = self
            .acceptor_for(context)?
            .accept(io)
            .await
            .map_err(tls_handshake)?;
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

#[derive(Clone)]
pub struct ClientTlsAdapter {
    connector: TlsConnector,
}

impl fmt::Debug for ClientTlsAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientTlsAdapter")
            .finish_non_exhaustive()
    }
}

impl ClientTlsAdapter {
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
        Ok(Self {
            connector: TlsConnector::from(Arc::new(config)),
        })
    }

    /// The owned `ServerName` enables both real SNI and `WebPKI` hostname/IP verification.
    pub async fn connect(&self, domain: &str, io: BoxIo) -> Result<BoxIo> {
        let server_name = ServerName::try_from(domain.to_owned()).map_err(tls_config)?;
        let stream = self
            .connector
            .connect(server_name, io)
            .await
            .map_err(tls_handshake)?;
        Ok(Box::new(stream))
    }
}

fn private_key(bytes: Vec<u8>) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(bytes))
}

fn certified_key(
    certificate_chain: Vec<Vec<u8>>,
    private_key_bytes: Vec<u8>,
) -> Result<CertifiedKey> {
    let certificates = certificate_chain
        .into_iter()
        .map(CertificateDer::from)
        .collect();
    let mut private_key = private_key(private_key_bytes);
    let signing_key = rustls::crypto::ring::sign::any_supported_type(&private_key);
    private_key.zeroize();
    let certified_key = CertifiedKey::new(certificates, signing_key.map_err(tls_config)?);
    certified_key.keys_match().map_err(tls_config)?;
    Ok(certified_key)
}

fn peer_identity(certificate_der: &[u8]) -> Result<TlsPeerIdentity> {
    let (remaining, certificate) = parse_x509_certificate(certificate_der).map_err(|error| {
        ProxyError::new(
            ErrorCode::TlsHandshakeFailed,
            format!("client certificate is invalid: {error:?}"),
        )
    })?;
    if !remaining.is_empty() {
        return Err(ProxyError::new(
            ErrorCode::TlsHandshakeFailed,
            "client certificate contains trailing DER data",
        ));
    }
    Ok(TlsPeerIdentity {
        sha256_fingerprint: digest(&SHA256, certificate_der)
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":"),
        subject_summary: certificate.subject().to_string(),
    })
}

fn application_verification_failure() -> Error {
    Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
}

fn tls_config(error: impl fmt::Display) -> ProxyError {
    ProxyError::new(ErrorCode::ConfigInvalid, error.to_string())
}

fn tls_handshake(error: impl fmt::Display) -> ProxyError {
    ProxyError::new(ErrorCode::TlsHandshakeFailed, error.to_string())
}
