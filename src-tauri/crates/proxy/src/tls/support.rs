use std::fmt;

use ring::digest::{SHA256, digest};
use rustls::{
    CertificateError, Error,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    sign::CertifiedKey,
};
use x509_parser::parse_x509_certificate;
use zeroize::Zeroize;

use crate::transport::TlsPeerIdentity;
use crate::{ErrorCode, ProxyError, Result};

pub(super) fn certified_key(
    certificate_chain: Vec<Vec<u8>>,
    private_key_bytes: Vec<u8>,
) -> Result<CertifiedKey> {
    let certificates = certificate_chain
        .into_iter()
        .map(CertificateDer::from)
        .collect();
    let mut private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key_bytes));
    let signing_key = rustls::crypto::ring::sign::any_supported_type(&private_key);
    private_key.zeroize();
    let certified_key = CertifiedKey::new(certificates, signing_key.map_err(tls_config)?);
    certified_key.keys_match().map_err(tls_config)?;
    Ok(certified_key)
}

pub(super) fn peer_identity(certificate_der: &[u8]) -> Result<TlsPeerIdentity> {
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

pub(super) fn application_verification_failure() -> Error {
    Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
}

pub(super) fn tls_config(error: impl fmt::Display) -> ProxyError {
    ProxyError::new(ErrorCode::ConfigInvalid, error.to_string())
}

pub(super) fn tls_handshake(error: impl fmt::Display) -> ProxyError {
    ProxyError::new(ErrorCode::TlsHandshakeFailed, error.to_string())
}
