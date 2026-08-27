use intercept_proxy_application::{
    AppError, AppResult, CertificateReference, CertificateReferenceKind,
};
use intercept_proxy_runtime::ReverseClientIdentity;

use crate::CertificateService;

use super::{
    KIND_DOWNSTREAM_CLIENT_TRUST, KIND_DOWNSTREAM_SERVER_IDENTITY, KIND_UPSTREAM_CLIENT_IDENTITY,
    KIND_UPSTREAM_CLIENT_IDENTITY_PEM, KIND_UPSTREAM_SERVER_TRUST,
    ManagedListenerCertificateAdapter, app_error, kind_mismatch, managed_key,
};

impl ManagedListenerCertificateAdapter {
    pub async fn resolve_trust(
        &self,
        reference: &CertificateReference,
    ) -> Option<AppResult<Vec<Vec<u8>>>> {
        let key = match managed_key(&reference.reference)? {
            Ok(key) => key,
            Err(error) => return Some(Err(error)),
        };
        Some(self.load_async(key).await.and_then(|material| {
            let trusted = match (reference.kind, material.kind) {
                (CertificateReferenceKind::UpstreamServerTrust, KIND_UPSTREAM_SERVER_TRUST) => {
                    CertificateService.parse_upstream_ca(&material.bytes)
                }
                (CertificateReferenceKind::DownstreamClientTrust, KIND_DOWNSTREAM_CLIENT_TRUST) => {
                    CertificateService.parse_client_trust_anchor(&material.bytes)
                }
                _ => return Err(kind_mismatch()),
            }
            .map_err(app_error)?;
            Ok(trusted.certificate_chain_der)
        }))
    }

    pub async fn resolve_identity(
        &self,
        reference: &CertificateReference,
    ) -> Option<AppResult<ReverseClientIdentity>> {
        let key = match managed_key(&reference.reference)? {
            Ok(key) => key,
            Err(error) => return Some(Err(error)),
        };
        Some(self.load_async(key).await.and_then(|material| {
            match (reference.kind, material.kind) {
                (
                    CertificateReferenceKind::UpstreamClientIdentity,
                    KIND_UPSTREAM_CLIENT_IDENTITY,
                ) => {
                    let password = std::str::from_utf8(&material.password).map_err(|_| {
                        AppError::new("CERTIFICATE_NOT_READY", "受保护的 PKCS12 密码编码无效。")
                    })?;
                    let mut parsed = CertificateService
                        .parse_pkcs12(&material.bytes, password)
                        .map_err(app_error)?;
                    let mut chain = vec![std::mem::take(&mut parsed.certificate_der)];
                    chain.extend(std::mem::take(&mut parsed.chain_der));
                    Ok(ReverseClientIdentity {
                        certificate_chain_der: chain,
                        private_key_pkcs8_der: std::mem::take(&mut parsed.private_key_pkcs8_der),
                    })
                }
                (
                    CertificateReferenceKind::UpstreamClientIdentity,
                    KIND_UPSTREAM_CLIENT_IDENTITY_PEM,
                ) => {
                    let mut parsed = CertificateService
                        .parse_client_identity_pem(&material.bytes)
                        .map_err(app_error)?;
                    Ok(ReverseClientIdentity {
                        certificate_chain_der: std::mem::take(&mut parsed.certificate_chain_der),
                        private_key_pkcs8_der: std::mem::take(&mut parsed.private_key_pkcs8_der),
                    })
                }
                (
                    CertificateReferenceKind::ReverseServerIdentity,
                    KIND_DOWNSTREAM_SERVER_IDENTITY,
                ) => {
                    let mut parsed = CertificateService
                        .parse_server_identity_pem(&material.bytes, "")
                        .map_err(app_error)?;
                    Ok(ReverseClientIdentity {
                        certificate_chain_der: std::mem::take(&mut parsed.certificate_chain_der),
                        private_key_pkcs8_der: std::mem::take(&mut parsed.private_key_pkcs8_der),
                    })
                }
                _ => Err(kind_mismatch()),
            }
        }))
    }
}
