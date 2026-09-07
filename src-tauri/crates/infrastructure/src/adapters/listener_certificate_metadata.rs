//! Listener 证书安全引用的只读元数据解析。
//!
//! 这里仅提取公开证书字段；不会把文件路径、原始证书、密码或私钥返回给应用层。

use std::path::PathBuf;

use crate::{CertificateMetadata, CertificateService};
use chrono::{DateTime, NaiveDateTime, Utc};
use intercept_proxy_application::{
    AppError, AppResult, CertificateItemViewModel, CertificateReference, CertificateReferenceKind,
    UiTone,
};

use super::{common::app_error, listener_certificate_store::read_secret_file};

pub(super) fn inspect_file_reference(
    reference: &CertificateReference,
) -> AppResult<CertificateItemViewModel> {
    let (path, password_environment) = identity_reference(&reference.reference)?;
    let bytes = read_secret_file(&path)?;
    let service = CertificateService;
    let metadata = match reference.kind {
        CertificateReferenceKind::UpstreamClientIdentity if password_environment.is_some() => {
            let variable = password_environment.expect("checked above");
            let password = zeroize::Zeroizing::new(std::env::var(&variable).map_err(|_| {
                AppError::new(
                    "CERTIFICATE_NOT_READY",
                    format!("PKCS12 密码环境变量 {variable} 未设置。"),
                )
            })?);
            service
                .parse_pkcs12(&bytes, password.as_str())
                .map_err(app_error)?
                .metadata
                .clone()
        }
        CertificateReferenceKind::DownstreamClientTrust => {
            service
                .parse_client_trust_anchor(&bytes)
                .map_err(app_error)?
                .metadata
        }
        CertificateReferenceKind::UpstreamServerTrust => {
            service
                .parse_upstream_ca(&bytes)
                .map_err(app_error)?
                .metadata
        }
        _ => service.inspect_certificate(&bytes).map_err(app_error)?,
    };
    Ok(view_model(reference.kind, metadata))
}

pub(super) fn view_model(
    kind: CertificateReferenceKind,
    metadata: CertificateMetadata,
) -> CertificateItemViewModel {
    let valid_from = parse_validity_date(&metadata.not_before);
    let valid_until = parse_validity_date(&metadata.not_after);
    let now = Utc::now();
    let (status_text, ui_tone) = match (valid_from, valid_until) {
        (Some(start), Some(end)) if now < start || now > end => {
            ("证书无效或已过期".into(), UiTone::Danger)
        }
        (_, Some(end)) if end < now + chrono::Duration::days(60) => {
            ("将在 60 天内到期".into(), UiTone::Warning)
        }
        (Some(_), Some(_)) => ("有效".into(), UiTone::Positive),
        _ => ("有效期无法解析".into(), UiTone::Warning),
    };
    CertificateItemViewModel {
        kind: kind_name(kind).into(),
        subject: metadata.subject,
        usage: usage(kind).into(),
        sans: metadata.san,
        valid_from,
        valid_until,
        sha256_fingerprint: metadata.fingerprint_sha256,
        status_text,
        ui_tone,
    }
}

fn identity_reference(reference: &str) -> AppResult<(PathBuf, Option<String>)> {
    if let Some(value) = reference.strip_prefix("pkcs12:") {
        let (path, query) = value.split_once('?').ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_NOT_READY",
                "PKCS12 引用必须提供 ?password_env=环境变量名。",
            )
        })?;
        let variable = query
            .strip_prefix("password_env=")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::new("CERTIFICATE_NOT_READY", "PKCS12 引用的 password_env 无效。")
            })?;
        return Ok((PathBuf::from(path), Some(variable.to_owned())));
    }
    let value = reference.strip_prefix("file:").unwrap_or(reference);
    if value.trim().is_empty() {
        return Err(AppError::new("CERTIFICATE_NOT_READY", "证书安全引用为空。"));
    }
    Ok((PathBuf::from(value), None))
}

fn parse_validity_date(value: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(value, "%b %e %H:%M:%S %Y GMT")
        .ok()
        .map(|value| value.and_utc())
}

const fn kind_name(kind: CertificateReferenceKind) -> &'static str {
    match kind {
        CertificateReferenceKind::MitmRootCa => "mitm_root_ca",
        CertificateReferenceKind::ReverseServerIdentity => "reverse_server_identity",
        CertificateReferenceKind::DownstreamClientTrust => "downstream_client_trust",
        CertificateReferenceKind::UpstreamClientIdentity => "upstream_client_identity",
        CertificateReferenceKind::UpstreamServerTrust => "upstream_server_trust",
    }
}

const fn usage(kind: CertificateReferenceKind) -> &'static str {
    match kind {
        CertificateReferenceKind::MitmRootCa => "正向代理 MITM Root CA",
        CertificateReferenceKind::ReverseServerIdentity => "代理向客户端出示的 TLS 服务端身份",
        CertificateReferenceKind::DownstreamClientTrust => "验证客户端证书的 CA",
        CertificateReferenceKind::UpstreamClientIdentity => {
            "代理向上游服务器出示的 mTLS 客户端身份"
        }
        CertificateReferenceKind::UpstreamServerTrust => "验证上游服务器证书的 CA",
    }
}
