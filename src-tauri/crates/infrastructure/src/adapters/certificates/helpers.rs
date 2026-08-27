use super::{
    AppError, AppResult, CertificateItemViewModel, DateTime, IpAddr, LeafCertificateRequest,
    MaterialStatus, NaiveDateTime, ProtectedMaterial, ProxyError, ProxyErrorCode, TimeZone, UiTone,
    Utc,
};

pub(super) fn from_bundle(revision: u64, bundle: &crate::CertificateBundle) -> ProtectedMaterial {
    ProtectedMaterial {
        revision,
        certificate_der: bundle.certificate_der.clone(),
        private_key_der: bundle.private_key_pkcs8_der.to_vec(),
        chain_der: Vec::new(),
        subject: bundle.metadata.subject.clone(),
        fingerprint: bundle.metadata.fingerprint_sha256.clone(),
        sans: bundle.metadata.san.clone(),
        not_before: bundle.metadata.not_before.clone(),
        not_after: bundle.metadata.not_after.clone(),
    }
}

pub(super) fn leaf_request(sans: &[String]) -> AppResult<LeafCertificateRequest> {
    if sans.is_empty() {
        return Err(AppError::new(
            "CERTIFICATE_INVALID",
            "叶子证书至少需要一个 LAN IP 或 DNS SAN。",
        ));
    }
    let mut dns_names = Vec::new();
    let mut ip_addresses = Vec::new();
    for san in sans {
        match san.parse::<IpAddr>() {
            Ok(address) => ip_addresses.push(address),
            Err(_) => dns_names.push(san.clone()),
        }
    }
    Ok(LeafCertificateRequest {
        common_name: sans[0].clone(),
        dns_names,
        ip_addresses,
    })
}

pub(super) fn item(
    kind: &str,
    usage: &str,
    material: &ProtectedMaterial,
) -> CertificateItemViewModel {
    let now = Utc::now();
    let (status_text, ui_tone) = match (
        parse_validity_date(&material.not_before),
        parse_validity_date(&material.not_after),
    ) {
        (Some(not_before), Some(not_after)) if now < not_before || now > not_after => {
            ("证书无效或已过期".into(), UiTone::Danger)
        }
        (_, Some(not_after)) if not_after < now + chrono::Duration::days(60) => {
            ("将在 60 天内到期".into(), UiTone::Warning)
        }
        (Some(_), Some(_)) => ("有效".into(), UiTone::Positive),
        _ => ("证书无效".into(), UiTone::Danger),
    };
    CertificateItemViewModel {
        // `kind` 是前端筛选与未来 TUI/CLI 共用的稳定标识，不能写入会变化的中文
        // 显示名称。用户可见文案由 `usage` 承载。
        kind: kind.into(),
        subject: material.subject.clone(),
        usage: usage.into(),
        sans: material.sans.clone(),
        valid_from: parse_validity_date(&material.not_before),
        valid_until: parse_validity_date(&material.not_after),
        sha256_fingerprint: material.fingerprint.clone(),
        status_text,
        ui_tone,
    }
}

pub(super) fn status_item(
    kind: &str,
    usage: &str,
    status: &MaterialStatus,
) -> CertificateItemViewModel {
    let valid_from = status.not_before.as_deref().and_then(parse_validity_date);
    let valid_until = status.not_after.as_deref().and_then(parse_validity_date);
    let now = Utc::now();
    let (status_text, ui_tone) = match (valid_from, valid_until) {
        (Some(not_before), Some(not_after)) if now < not_before || now > not_after => {
            ("证书无效或已过期".into(), UiTone::Danger)
        }
        (_, Some(not_after)) if not_after < now + chrono::Duration::days(60) => {
            ("将在 60 天内到期".into(), UiTone::Warning)
        }
        (Some(_), Some(_)) => ("有效".into(), UiTone::Positive),
        _ => ("已配置，需重新校验".into(), UiTone::Warning),
    };
    CertificateItemViewModel {
        kind: kind.into(),
        subject: status.subject.clone(),
        usage: usage.into(),
        sans: status.sans.clone(),
        valid_from,
        valid_until,
        sha256_fingerprint: status.fingerprint.clone(),
        status_text,
        ui_tone,
    }
}

pub(super) fn material_matches(
    material: &ProtectedMaterial,
    metadata: &crate::CertificateMetadata,
) -> bool {
    material.subject == metadata.subject
        && material.fingerprint == metadata.fingerprint_sha256
        && material.sans == metadata.san
        && material.not_before == metadata.not_before
        && material.not_after == metadata.not_after
}

pub(super) fn parse_validity_date(value: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(value, "%b %e %H:%M:%S %Y GMT")
        .ok()
        .map(|value| Utc.from_utc_datetime(&value))
}

pub(super) fn verify_revision(actual: u64, expected: u64) -> AppResult<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(AppError::new(
            "REVISION_CONFLICT",
            "证书配置已被其他操作更新。",
        ))
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn proxy_infra_error(error: crate::InfrastructureError) -> ProxyError {
    ProxyError::new(ProxyErrorCode::CertificateInvalid, error.to_string())
}
