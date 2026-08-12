//! 可移植 Listener 证书材料的类型校验与规范化。

use intercept_proxy_application::{
    AppError, AppResult, CertificateReferenceKind, PortableCertificateMaterial,
};

use super::{
    KIND_DOWNSTREAM_CLIENT_TRUST, KIND_DOWNSTREAM_SERVER_IDENTITY, KIND_UPSTREAM_CLIENT_IDENTITY,
    KIND_UPSTREAM_CLIENT_IDENTITY_PEM, KIND_UPSTREAM_SERVER_TRUST,
};
use crate::{CertificateService, adapters::common::app_error};

pub(super) fn validate_portable_material(
    material: &PortableCertificateMaterial,
    bytes: &[u8],
) -> AppResult<(u8, String, Vec<u8>)> {
    match material.kind {
        CertificateReferenceKind::UpstreamClientIdentity => {
            if let Some(password) = material.password.clone() {
                CertificateService
                    .parse_pkcs12(bytes, &password)
                    .map_err(app_error)?;
                Ok((KIND_UPSTREAM_CLIENT_IDENTITY, password, bytes.to_vec()))
            } else {
                CertificateService
                    .parse_client_identity_pem(bytes)
                    .map_err(app_error)?;
                Ok((
                    KIND_UPSTREAM_CLIENT_IDENTITY_PEM,
                    String::new(),
                    bytes.to_vec(),
                ))
            }
        }
        CertificateReferenceKind::UpstreamServerTrust => {
            let parsed = CertificateService
                .parse_upstream_ca(bytes)
                .map_err(app_error)?;
            Ok((
                KIND_UPSTREAM_SERVER_TRUST,
                String::new(),
                parsed.certificate_der,
            ))
        }
        CertificateReferenceKind::ReverseServerIdentity => {
            CertificateService
                .parse_server_identity_pem(bytes)
                .map_err(app_error)?;
            Ok((
                KIND_DOWNSTREAM_SERVER_IDENTITY,
                String::new(),
                bytes.to_vec(),
            ))
        }
        CertificateReferenceKind::DownstreamClientTrust => {
            let parsed = CertificateService
                .parse_client_trust_anchor(bytes)
                .map_err(app_error)?;
            Ok((
                KIND_DOWNSTREAM_CLIENT_TRUST,
                String::new(),
                parsed.certificate_der,
            ))
        }
        CertificateReferenceKind::MitmRootCa => Err(AppError::new(
            "PORTABLE_CERTIFICATE_INVALID",
            "本机 MITM Root CA 不属于 Listener 可移植证书材料。",
        )
        .entity(material.reference_id.to_string())),
    }
}
