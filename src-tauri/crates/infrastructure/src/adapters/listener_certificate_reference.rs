use intercept_proxy_application::{
    AppError, AppResult, CertificateItemViewModel, CertificateReference, CertificateReferenceId,
    CertificateReferenceKind, ListenerCertificateDetailViewModel,
    ListenerCertificateImportViewModel,
};

use super::{
    KIND_DOWNSTREAM_CLIENT_TRUST, KIND_DOWNSTREAM_SERVER_IDENTITY, KIND_UPSTREAM_CLIENT_IDENTITY,
    KIND_UPSTREAM_CLIENT_IDENTITY_PEM, KIND_UPSTREAM_SERVER_TRUST, REFERENCE_PREFIX,
};

pub(super) fn reference(
    label: String,
    kind: CertificateReferenceKind,
    key: &str,
) -> CertificateReference {
    CertificateReference {
        id: CertificateReferenceId::new(),
        label,
        kind,
        reference: format!("{REFERENCE_PREFIX}{key}"),
    }
}

pub(super) fn imported(
    reference: CertificateReference,
    certificate: CertificateItemViewModel,
) -> ListenerCertificateImportViewModel {
    ListenerCertificateImportViewModel {
        detail: ListenerCertificateDetailViewModel {
            reference_id: reference.id,
            label: reference.label.clone(),
            certificate: Some(certificate),
            error_message: None,
        },
        reference,
    }
}

pub(super) fn kind_mismatch() -> AppError {
    AppError::new(
        "CERTIFICATE_NOT_READY",
        "Listener TLS 安全引用的材料类型不匹配。",
    )
}

pub(super) fn ensure_kind_matches(
    kind: CertificateReferenceKind,
    stored_kind: u8,
) -> AppResult<()> {
    let matches = matches!(
        (kind, stored_kind),
        (
            CertificateReferenceKind::UpstreamClientIdentity,
            KIND_UPSTREAM_CLIENT_IDENTITY | KIND_UPSTREAM_CLIENT_IDENTITY_PEM
        ) | (
            CertificateReferenceKind::UpstreamServerTrust,
            KIND_UPSTREAM_SERVER_TRUST
        ) | (
            CertificateReferenceKind::ReverseServerIdentity,
            KIND_DOWNSTREAM_SERVER_IDENTITY
        ) | (
            CertificateReferenceKind::DownstreamClientTrust,
            KIND_DOWNSTREAM_CLIENT_TRUST
        )
    );
    matches.then_some(()).ok_or_else(kind_mismatch)
}
