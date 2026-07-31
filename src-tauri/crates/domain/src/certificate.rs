//! 证书元数据与启动前校验规则。
//!
//! 本模块不读取文件，也不执行 TLS 握手。它只根据已解析的元数据判断有效期、用途和
//! 密钥保护状态，使桌面 UI、未来 TUI/CLI 与无界面测试复用同一套判断。

use crate::{CertificateId, DomainError, ErrorCode, Revision};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

/// 距离到期不足该天数时显示预警，但仍允许启动。
pub const CERTIFICATE_EXPIRY_WARNING_DAYS: i64 = 60;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 代理涉及的四类证书角色。
pub enum CertificateKind {
    LocalRootCa,
    ProxyLeaf,
    SharedClientIdentity,
    UpstreamCa,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 证书在指定时间点的可用状态。
pub enum CertificateStatus {
    Missing,
    Valid,
    ExpiringSoon,
    NotYetValid,
    Expired,
    Invalid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 不含私钥的证书摘要，可安全用于状态展示和持久化。
pub struct CertificateMetadata {
    pub id: CertificateId,
    pub revision: Revision,
    pub kind: CertificateKind,
    pub subject: String,
    pub issuer: String,
    pub sans: Vec<String>,
    pub sha256_fingerprint: String,
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub status: CertificateStatus,
}

impl CertificateMetadata {
    #[must_use]
    pub fn status_at(&self, now: DateTime<Utc>) -> CertificateStatus {
        if now < self.valid_from {
            CertificateStatus::NotYetValid
        } else if now >= self.valid_until {
            CertificateStatus::Expired
        } else if self.valid_until - now <= Duration::days(CERTIFICATE_EXPIRY_WARNING_DAYS) {
            CertificateStatus::ExpiringSoon
        } else {
            CertificateStatus::Valid
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 启动代理所需的完整证书清单。
pub struct CertificateInventory {
    pub local_root_ca: Option<CertificateMetadata>,
    pub proxy_leaf: Option<CertificateMetadata>,
    pub shared_client_identity: Option<CertificateMetadata>,
    pub upstream_ca: Option<CertificateMetadata>,
    pub allowed_client_fingerprints: Vec<String>,
}

impl CertificateInventory {
    pub fn validate_startup(
        &self,
        required_sans: &[String],
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        // 一次收集所有缺失/过期证书与 SAN 问题，让操作者无需反复“修一个、再校验”。
        let required = [
            ("local_root_ca", self.local_root_ca.as_ref()),
            ("proxy_leaf", self.proxy_leaf.as_ref()),
            (
                "shared_client_identity",
                self.shared_client_identity.as_ref(),
            ),
            ("upstream_ca", self.upstream_ca.as_ref()),
        ];
        let mut error =
            DomainError::new(ErrorCode::CertificateNotReady, "启动所需证书不完整或无效");
        for (field, certificate) in required {
            let Some(certificate) = certificate else {
                error = error.with_field_error(field, "证书尚未配置");
                continue;
            };
            if !matches!(
                certificate.status_at(now),
                CertificateStatus::Valid | CertificateStatus::ExpiringSoon
            ) {
                error = error.with_field_error(field, "证书不在有效期内");
            }
        }
        if let Some(leaf) = &self.proxy_leaf {
            for san in required_sans {
                if !leaf.sans.contains(san) {
                    error = error.with_field_error("proxy_leaf.sans", format!("缺少 SAN：{san}"));
                }
            }
        }
        if self.allowed_client_fingerprints.is_empty() {
            error = error.with_field_error(
                "allowed_client_fingerprints",
                "至少配置一个允许的客户端证书指纹",
            );
        }
        if error.field_errors.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct ProtectedSecretMetadata {
    pub certificate_id: CertificateId,
    pub protected_for_current_user: bool,
    pub revision: Revision,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn certificate(kind: CertificateKind, now: DateTime<Utc>, days: i64) -> CertificateMetadata {
        CertificateMetadata {
            id: CertificateId::new(),
            revision: Revision::INITIAL,
            kind,
            subject: "CN=test".into(),
            issuer: "CN=root".into(),
            sans: vec!["192.168.1.10".into()],
            sha256_fingerprint: "AA:BB".into(),
            valid_from: now - Duration::days(1),
            valid_until: now + Duration::days(days),
            status: CertificateStatus::Valid,
        }
    }

    // CERT-012, CERT-014
    #[test]
    fn certificate_metadata_exposes_no_secret_and_warns_sixty_days_before_expiry() {
        let now = Utc::now();
        let metadata = certificate(CertificateKind::ProxyLeaf, now, 60);
        assert_eq!(metadata.status_at(now), CertificateStatus::ExpiringSoon);
        let serialized = serde_json::to_string(&metadata).unwrap();
        assert!(!serialized.contains("password"));
        assert!(!serialized.contains("private_key"));
    }

    // CERT-013, CERT-017
    #[test]
    fn startup_validation_fails_closed_for_missing_material_and_san() {
        let now = Utc::now();
        let inventory = CertificateInventory {
            local_root_ca: Some(certificate(CertificateKind::LocalRootCa, now, 365)),
            proxy_leaf: Some(certificate(CertificateKind::ProxyLeaf, now, 365)),
            shared_client_identity: None,
            upstream_ca: None,
            allowed_client_fingerprints: Vec::new(),
        };
        let error = inventory
            .validate_startup(&["proxy.example.test".into()], now)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::CertificateNotReady);
        assert!(error.field_errors.len() >= 3);
    }
}
