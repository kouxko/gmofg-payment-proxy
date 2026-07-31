//! 证书应用服务适配器：连接持久化密文、系统密钥保护器与证书解析器。
//!
//! 私钥只在导入、签发或构建 TLS 快照时短暂解密；数据库保存的是受当前用户保护的密文。
//! 任一步失败都不发布部分证书快照，避免证书与私钥来自不同版本。

use std::{collections::BTreeMap, fmt, net::IpAddr, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use gmofg_proxy_application::{
    AppError, AppResult, CertificateItemViewModel, CertificateOverviewViewModel,
    CertificateServicePort, CertificateValidationViewModel, FieldValidationViewModel,
    OperationResultViewModel, UiTone,
};
use gmofg_proxy_product_api::{
    EmbeddedTestCertificateAuthority, ProductCertificatePolicy, ProductProfile,
};
use gmofg_proxy_runtime::{
    ErrorCode as ProxyErrorCode, ProxyError, TlsMaterialProvider, TlsMaterialSnapshot,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::files::{CA_IMPORT_MAX_BYTES, PKCS12_IMPORT_MAX_BYTES};
use crate::{
    AtomicFileExporter, CertificateMaterialRecord, CertificateService, LeafCertificateRequest,
    SecretProtector, SqliteStore,
};

use super::{
    common::{app_error, infra, json_error},
    files::{NativeFileDialog, cancelled},
};

const ROOT: &str = "local_root_ca";
const LEAF: &str = "proxy_leaf";
const PKCS12: &str = "shared_pkcs12";
const UPSTREAM_CA: &str = "upstream_ca";
const MATERIAL_KINDS: [&str; 4] = [ROOT, LEAF, PKCS12, UPSTREAM_CA];

#[derive(Clone, Serialize, Deserialize)]
struct ProtectedMaterial {
    revision: u64,
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
    chain_der: Vec<Vec<u8>>,
    subject: String,
    fingerprint: String,
    sans: Vec<String>,
    not_before: String,
    not_after: String,
}

struct MaterialSnapshot {
    revision: u64,
    materials: BTreeMap<String, ProtectedMaterial>,
}

impl fmt::Debug for ProtectedMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProtectedMaterial")
            .field("revision", &self.revision)
            .field("subject", &self.subject)
            .field("fingerprint", &self.fingerprint)
            .field("sans", &self.sans)
            .field("secret_material", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl Drop for ProtectedMaterial {
    fn drop(&mut self) {
        self.private_key_der.zeroize();
        for certificate in &mut self.chain_der {
            certificate.zeroize();
        }
    }
}

pub struct CertificateServiceAdapter {
    store: Arc<SqliteStore>,
    protector: Arc<dyn SecretProtector>,
    dialog: Arc<dyn NativeFileDialog>,
    certificates: CertificateService,
    product: Arc<dyn ProductProfile>,
    exporter: AtomicFileExporter,
    material_lock: Mutex<()>,
}

impl fmt::Debug for CertificateServiceAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CertificateServiceAdapter")
            .field("protected_storage", &true)
            .field("secret_material", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl CertificateServiceAdapter {
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        protector: Arc<dyn SecretProtector>,
        dialog: Arc<dyn NativeFileDialog>,
        product: Arc<dyn ProductProfile>,
    ) -> Self {
        Self {
            store,
            protector,
            dialog,
            certificates: CertificateService,
            product,
            exporter: AtomicFileExporter,
            material_lock: Mutex::new(()),
        }
    }

    fn certificate_policy(&self) -> &dyn ProductCertificatePolicy {
        self.product.certificates()
    }

    fn embedded_test_authority(&self) -> AppResult<EmbeddedTestCertificateAuthority> {
        self.certificate_policy()
            .embedded_test_authority()
            .ok_or_else(|| {
                AppError::new(
                    "CERTIFICATE_TEST_SIGNING_DISABLED",
                    "当前产品配置未启用嵌入测试 Root CA 私钥，禁止生成或重签证书。",
                )
            })
    }

    fn load_snapshot(&self, kinds: &[&str]) -> AppResult<MaterialSnapshot> {
        let snapshot = infra(self.store.load_certificate_materials_snapshot(kinds))?;
        let mut materials = BTreeMap::new();
        for record in snapshot.records {
            let plaintext = Zeroizing::new(
                self.protector
                    .unprotect(&record.protected_blob)
                    .map_err(app_error)?,
            );
            let material: ProtectedMaterial = serde_json::from_slice(&plaintext)
                .map_err(|error| json_error("受保护证书材料无效", error))?;
            if record
                .metadata
                .get("revision")
                .and_then(serde_json::Value::as_u64)
                != Some(material.revision)
            {
                return Err(AppError::new(
                    "CERTIFICATE_INVALID",
                    "证书材料与元数据修订号不一致。",
                ));
            }
            materials.insert(record.kind, material);
        }
        Ok(MaterialSnapshot {
            revision: snapshot.revision,
            materials,
        })
    }

    fn record(
        &self,
        kind: &str,
        material: &ProtectedMaterial,
    ) -> AppResult<CertificateMaterialRecord> {
        let plaintext = Zeroizing::new(
            serde_json::to_vec(material)
                .map_err(|error| json_error("证书材料序列化失败", error))?,
        );
        let protected = self.protector.protect(&plaintext).map_err(app_error)?;
        Ok(CertificateMaterialRecord {
            kind: kind.into(),
            protected_blob: protected,
            metadata: serde_json::json!({
                "revision": material.revision,
                "subject": material.subject,
                "fingerprint": material.fingerprint,
                "sans": material.sans,
            }),
            updated_at: Utc::now(),
        })
    }

    fn commit_snapshot(&self, mut snapshot: MaterialSnapshot) -> AppResult<u64> {
        let next_revision = snapshot.revision.saturating_add(1);
        let records = snapshot
            .materials
            .iter_mut()
            .map(|(kind, material)| {
                material.revision = next_revision;
                self.record(kind, material)
            })
            .collect::<AppResult<Vec<_>>>()?;
        infra(
            self.store
                .compare_and_swap_certificate_materials(snapshot.revision, &records),
        )
    }

    fn generate_locked(&self, sans: &[String], mut snapshot: MaterialSnapshot) -> AppResult<u64> {
        let root = self
            .certificates
            .load_embedded_test_root(self.embedded_test_authority()?)
            .map_err(app_error)?;
        let request = leaf_request(sans)?;
        let leaf = self
            .certificates
            .generate_leaf(&root.certificate_der, &root.private_key_pkcs8_der, &request)
            .map_err(app_error)?;
        snapshot
            .materials
            .insert(ROOT.into(), from_bundle(snapshot.revision, &root));
        snapshot
            .materials
            .insert(LEAF.into(), from_bundle(snapshot.revision, &leaf));
        self.commit_snapshot(snapshot)
    }

    fn bundled_upstream_material(&self, revision: u64) -> AppResult<Option<ProtectedMaterial>> {
        let Some(certificates_pem) = self.certificate_policy().bundled_upstream_ca_pem() else {
            return Ok(None);
        };
        let bundled = self
            .certificates
            .load_bundled_upstream_ca(certificates_pem)
            .map_err(app_error)?;
        Ok(Some(ProtectedMaterial {
            revision,
            certificate_der: bundled.certificate_der,
            private_key_der: Vec::new(),
            chain_der: Vec::new(),
            subject: bundled.metadata.subject,
            fingerprint: bundled.metadata.fingerprint_sha256,
            sans: bundled.metadata.san,
            not_before: bundled.metadata.not_before,
            not_after: bundled.metadata.not_after,
        }))
    }

    fn overview_locked(&self) -> AppResult<CertificateOverviewViewModel> {
        let labels = self.certificate_policy().labels();
        let snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        let upstream_is_override = snapshot.materials.contains_key(UPSTREAM_CA);
        let upstream = match snapshot.materials.get(UPSTREAM_CA).cloned() {
            Some(material) => Some(material),
            None => self.bundled_upstream_material(snapshot.revision)?,
        };
        let materials = [
            (ROOT, labels.root_name, labels.root_usage),
            (LEAF, labels.leaf_name, labels.leaf_usage),
            (
                PKCS12,
                labels.client_identity_name,
                labels.client_identity_usage,
            ),
            (
                UPSTREAM_CA,
                labels.upstream_name,
                if upstream_is_override {
                    labels.upstream_override_usage
                } else {
                    labels.upstream_bundled_usage
                },
            ),
        ]
        .into_iter()
        .map(|(kind, display, usage)| {
            let material = if kind == UPSTREAM_CA {
                upstream.clone()
            } else {
                snapshot.materials.get(kind).cloned()
            };
            (kind, display, usage, material)
        })
        .collect::<Vec<_>>();
        let errors = self.configuration_errors(&materials);
        let ready = errors.is_empty();
        Ok(CertificateOverviewViewModel {
            revision: snapshot.revision,
            ready,
            status_text: if ready {
                labels.ready_status.into()
            } else {
                labels.incomplete_status.into()
            },
            ui_tone: if ready {
                UiTone::Positive
            } else {
                UiTone::Warning
            },
            items: materials
                .iter()
                .filter_map(|(_, display, usage, material)| {
                    material
                        .as_ref()
                        .map(|material| item(display, usage, material))
                })
                .collect(),
            can_initialize: !snapshot.materials.contains_key(ROOT)
                && !snapshot.materials.contains_key(LEAF),
            can_change: true,
            disabled_reason: None,
        })
    }

    fn configuration_errors(
        &self,
        materials: &[(&str, &str, &str, Option<ProtectedMaterial>)],
    ) -> std::collections::BTreeMap<String, Vec<String>> {
        let mut errors = std::collections::BTreeMap::new();
        let find = |kind: &str| {
            materials
                .iter()
                .find(|(candidate, _, _, _)| *candidate == kind)
                .and_then(|(_, _, _, material)| material.as_ref())
        };
        for kind in [ROOT, LEAF, PKCS12, UPSTREAM_CA] {
            if find(kind).is_none() {
                errors.insert(kind.into(), vec!["证书材料尚未配置。".into()]);
            }
        }
        let root_metadata = find(ROOT).and_then(|root| {
            let Ok(authority) = self.embedded_test_authority() else {
                errors.insert(ROOT.into(), vec!["Root CA 校验失败。".into()]);
                return None;
            };
            match self.certificates.validate_embedded_test_root(
                &root.certificate_der,
                &root.private_key_der,
                authority,
            ) {
                Ok(metadata) if material_matches(root, &metadata) => Some(metadata),
                Ok(_) | Err(_) => {
                    errors.insert(ROOT.into(), vec!["Root CA 校验失败。".into()]);
                    None
                }
            }
        });
        if let (Some(root), Some(leaf), Some(_)) = (find(ROOT), find(LEAF), root_metadata) {
            match self.certificates.validate_leaf(
                &root.certificate_der,
                &leaf.certificate_der,
                &leaf.private_key_der,
                &leaf.sans,
            ) {
                Ok(metadata) if material_matches(leaf, &metadata) => {}
                Ok(_) | Err(_) => {
                    errors.insert(LEAF.into(), vec!["Proxy 叶子证书校验失败。".into()]);
                }
            }
        }
        if let Some(shared) = find(PKCS12) {
            match self.certificates.validate_client_identity(
                &shared.certificate_der,
                &shared.private_key_der,
                &shared.chain_der,
            ) {
                Ok(metadata) if material_matches(shared, &metadata) => {}
                Ok(_) | Err(_) => {
                    errors.insert(PKCS12.into(), vec!["共享 PKCS12 校验失败。".into()]);
                }
            }
        }
        if let Some(upstream) = find(UPSTREAM_CA) {
            match self.certificates.validate_ca_der(&upstream.certificate_der) {
                Ok(metadata) if material_matches(upstream, &metadata) => {}
                Ok(_) | Err(_) => {
                    errors.insert(UPSTREAM_CA.into(), vec!["上游 CA 校验失败。".into()]);
                }
            }
        }
        errors
    }
}

#[async_trait]
impl CertificateServicePort for CertificateServiceAdapter {
    async fn overview(&self) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        self.overview_locked()
    }

    async fn generate_ca(&self, sans: Vec<String>) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        if snapshot.materials.contains_key(ROOT) || snapshot.materials.contains_key(LEAF) {
            let labels = self.certificate_policy().labels();
            return Err(AppError::new(
                "CERTIFICATE_ALREADY_EXISTS",
                labels.already_exists_message,
            ));
        }
        self.generate_locked(&sans, snapshot)?;
        self.overview_locked()
    }

    async fn export_ca(&self) -> AppResult<OperationResultViewModel> {
        let _guard = self.material_lock.lock();
        let labels = self.certificate_policy().labels();
        let Some(selection) = self.dialog.choose_save_file("root_ca")? else {
            return Ok(cancelled(labels.export_cancelled_message));
        };
        infra(self.exporter.write(
            &selection.path,
            self.certificate_policy().public_root_ca_pem(),
            selection.overwrite_confirmed,
        ))?;
        Ok(OperationResultViewModel::success(
            labels.export_success_message,
        ))
    }

    async fn reissue_leaf(
        &self,
        expected_revision: u64,
        sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let mut snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        verify_revision(snapshot.revision, expected_revision)?;
        let root = self
            .certificates
            .load_embedded_test_root(self.embedded_test_authority()?)
            .map_err(app_error)?;
        let request = leaf_request(&sans)?;
        let leaf = self
            .certificates
            .generate_leaf(&root.certificate_der, &root.private_key_pkcs8_der, &request)
            .map_err(app_error)?;
        snapshot.materials.insert(
            ROOT.into(),
            from_bundle(snapshot.revision.saturating_add(1), &root),
        );
        snapshot.materials.insert(
            LEAF.into(),
            from_bundle(snapshot.revision.saturating_add(1), &leaf),
        );
        self.commit_snapshot(snapshot)?;
        self.overview_locked()
    }

    async fn import_pkcs12(&self, password: String) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let Some(path) = self.dialog.choose_open_file("pkcs12")? else {
            return self.overview_locked();
        };
        let password = Zeroizing::new(password);
        let bytes = Zeroizing::new(infra(
            self.exporter.read_bounded(&path, PKCS12_IMPORT_MAX_BYTES),
        )?);
        let parsed = self
            .certificates
            .parse_pkcs12(&bytes, &password)
            .map_err(app_error)?;
        let mut snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        snapshot.materials.insert(
            PKCS12.into(),
            ProtectedMaterial {
                revision: snapshot.revision.saturating_add(1),
                certificate_der: parsed.certificate_der.clone(),
                private_key_der: parsed.private_key_pkcs8_der.to_vec(),
                chain_der: parsed.chain_der.clone(),
                subject: parsed.metadata.subject.clone(),
                fingerprint: parsed.metadata.fingerprint_sha256.clone(),
                sans: parsed.metadata.san.clone(),
                not_before: parsed.metadata.not_before.clone(),
                not_after: parsed.metadata.not_after.clone(),
            },
        );
        self.commit_snapshot(snapshot)?;
        self.overview_locked()
    }

    async fn import_upstream_ca(&self) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let Some(path) = self.dialog.choose_open_file("upstream_ca")? else {
            return self.overview_locked();
        };
        let bytes = infra(self.exporter.read_bounded(&path, CA_IMPORT_MAX_BYTES))?;
        let parsed = self
            .certificates
            .parse_upstream_ca(&bytes)
            .map_err(app_error)?;
        let mut snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        snapshot.materials.insert(
            UPSTREAM_CA.into(),
            ProtectedMaterial {
                revision: snapshot.revision.saturating_add(1),
                certificate_der: parsed.certificate_der,
                private_key_der: Vec::new(),
                chain_der: Vec::new(),
                subject: parsed.metadata.subject,
                fingerprint: parsed.metadata.fingerprint_sha256,
                sans: parsed.metadata.san,
                not_before: parsed.metadata.not_before,
                not_after: parsed.metadata.not_after,
            },
        );
        self.commit_snapshot(snapshot)?;
        self.overview_locked()
    }

    async fn validate(&self) -> AppResult<CertificateValidationViewModel> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        let upstream = match snapshot.materials.get(UPSTREAM_CA).cloned() {
            Some(material) => Some(material),
            None => self.bundled_upstream_material(snapshot.revision)?,
        };
        let materials = MATERIAL_KINDS
            .into_iter()
            .map(|kind| {
                let material = if kind == UPSTREAM_CA {
                    upstream.clone()
                } else {
                    snapshot.materials.get(kind).cloned()
                };
                (kind, "", "", material)
            })
            .collect::<Vec<_>>();
        let field_errors = self.configuration_errors(&materials);
        Ok(FieldValidationViewModel {
            valid: field_errors.is_empty(),
            field_errors,
            warnings: Vec::new(),
        })
    }

    async fn reset_ca(&self, expected_revision: u64) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&MATERIAL_KINDS)?;
        verify_revision(snapshot.revision, expected_revision)?;
        let sans = snapshot
            .materials
            .get(LEAF)
            .map(|leaf| {
                leaf.sans
                    .iter()
                    .map(|san| {
                        san.trim_start_matches("DNS:")
                            .trim_start_matches("IP:")
                            .to_owned()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.generate_locked(&sans, snapshot)?;
        self.overview_locked()
    }
}

#[async_trait]
impl TlsMaterialProvider for CertificateServiceAdapter {
    async fn load_epoch_snapshot(
        &self,
        leaf_sans: &[String],
    ) -> gmofg_proxy_runtime::Result<TlsMaterialSnapshot> {
        let _guard = self.material_lock.lock();
        let snapshot = self
            .load_snapshot(&MATERIAL_KINDS)
            .map_err(proxy_app_error)?;
        let leaf = snapshot.materials.get(LEAF).ok_or_else(|| {
            ProxyError::new(
                ProxyErrorCode::CertificateNotReady,
                "Proxy leaf certificate missing",
            )
        })?;
        let root = snapshot.materials.get(ROOT).ok_or_else(|| {
            ProxyError::new(
                ProxyErrorCode::CertificateNotReady,
                "shared test Root CA missing",
            )
        })?;
        self.certificates
            .validate_embedded_test_root(
                &root.certificate_der,
                &root.private_key_der,
                self.embedded_test_authority().map_err(proxy_app_error)?,
            )
            .and_then(|_| {
                self.certificates.validate_leaf(
                    &root.certificate_der,
                    &leaf.certificate_der,
                    &leaf.private_key_der,
                    leaf_sans,
                )
            })
            .map_err(proxy_infra_error)?;
        let shared = snapshot.materials.get(PKCS12).ok_or_else(|| {
            ProxyError::new(ProxyErrorCode::CertificateNotReady, "shared PKCS12 missing")
        })?;
        let upstream = match snapshot.materials.get(UPSTREAM_CA).cloned() {
            Some(material) => material,
            None => self
                .bundled_upstream_material(snapshot.revision)
                .map_err(proxy_app_error)?
                .ok_or_else(|| {
                    ProxyError::new(ProxyErrorCode::CertificateNotReady, "upstream CA missing")
                })?,
        };
        let app_client_ca_der = shared
            .chain_der
            .last()
            .cloned()
            .unwrap_or_else(|| shared.certificate_der.clone());
        self.certificates
            .validate_client_identity(
                &shared.certificate_der,
                &shared.private_key_der,
                &shared.chain_der,
            )
            .map_err(proxy_infra_error)?;
        self.certificates
            .validate_client_trust_anchor_der(&app_client_ca_der)
            .map_err(proxy_infra_error)?;
        self.certificates
            .validate_ca_der(&upstream.certificate_der)
            .map_err(proxy_infra_error)?;
        Ok(TlsMaterialSnapshot {
            server_certificate_chain_der: vec![leaf.certificate_der.clone()],
            server_private_key_pkcs8_der: Zeroizing::new(leaf.private_key_der.clone()),
            app_client_ca_der,
            allowed_app_client_fingerprint: Some(parse_fingerprint(&shared.fingerprint)?),
            upstream_client_certificate_chain_der: std::iter::once(shared.certificate_der.clone())
                .chain(shared.chain_der.clone())
                .collect(),
            upstream_client_private_key_pkcs8_der: Zeroizing::new(shared.private_key_der.clone()),
            upstream_ca_der: upstream.certificate_der.clone(),
        })
    }
}

fn from_bundle(revision: u64, bundle: &crate::CertificateBundle) -> ProtectedMaterial {
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

fn leaf_request(sans: &[String]) -> AppResult<LeafCertificateRequest> {
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

fn item(name: &str, usage: &str, material: &ProtectedMaterial) -> CertificateItemViewModel {
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
        kind: name.into(),
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

fn material_matches(material: &ProtectedMaterial, metadata: &crate::CertificateMetadata) -> bool {
    material.subject == metadata.subject
        && material.fingerprint == metadata.fingerprint_sha256
        && material.sans == metadata.san
        && material.not_before == metadata.not_before
        && material.not_after == metadata.not_after
}

fn parse_validity_date(value: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(value, "%b %e %H:%M:%S %Y GMT")
        .ok()
        .map(|value| Utc.from_utc_datetime(&value))
}

fn verify_revision(actual: u64, expected: u64) -> AppResult<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(AppError::new(
            "REVISION_CONFLICT",
            "证书配置已被其他操作更新。",
        ))
    }
}

fn parse_fingerprint(value: &str) -> gmofg_proxy_runtime::Result<Vec<u8>> {
    value
        .split(':')
        .map(|part| {
            u8::from_str_radix(part, 16).map_err(|error| {
                ProxyError::new(ProxyErrorCode::CertificateInvalid, error.to_string())
            })
        })
        .collect()
}

#[allow(clippy::needless_pass_by_value)]
fn proxy_app_error(error: AppError) -> ProxyError {
    let code = match error.view_model.code.as_str() {
        "DPAPI_UNPROTECT_FAILED" => ProxyErrorCode::DpapiUnprotectFailed,
        "KEYCHAIN_UNPROTECT_FAILED" => ProxyErrorCode::KeychainUnprotectFailed,
        "CERTIFICATE_INVALID" => ProxyErrorCode::CertificateInvalid,
        "CERTIFICATE_NOT_READY" => ProxyErrorCode::CertificateNotReady,
        _ => ProxyErrorCode::Internal,
    };
    ProxyError::new(code, error.view_model.message.clone())
}

#[allow(clippy::needless_pass_by_value)]
fn proxy_infra_error(error: crate::InfrastructureError) -> ProxyError {
    ProxyError::new(ProxyErrorCode::CertificateInvalid, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, path::PathBuf, sync::Arc};

    use p12_keystore::{Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain};
    use parking_lot::Mutex as ParkingMutex;
    use rcgen::{
        CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer,
        KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
    };

    use super::*;
    use crate::adapters::{FileSelection, NativeFileDialog};
    use crate::{InfrastructureError, SecretProtector};
    use gmofg_proxy_product_payment::PaymentProductProfile;

    fn payment_profile() -> Arc<dyn ProductProfile> {
        Arc::new(PaymentProductProfile::isolated_test_tool())
    }

    #[derive(Debug)]
    struct QueueDialog {
        open: ParkingMutex<VecDeque<PathBuf>>,
    }

    impl NativeFileDialog for QueueDialog {
        fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
            Ok(self.open.lock().pop_front())
        }

        fn choose_save_file(&self, _: &str) -> AppResult<Option<FileSelection>> {
            Ok(None)
        }
    }

    #[derive(Debug)]
    struct ExportDialog {
        selection: ParkingMutex<Option<FileSelection>>,
    }

    impl NativeFileDialog for ExportDialog {
        fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
            Ok(None)
        }

        fn choose_save_file(&self, purpose: &str) -> AppResult<Option<FileSelection>> {
            assert_eq!(purpose, "root_ca");
            Ok(self.selection.lock().take())
        }
    }

    #[derive(Debug)]
    struct XorProtector;

    impl SecretProtector for XorProtector {
        fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            Ok(plaintext.iter().map(|byte| byte ^ 0xA5).collect())
        }

        fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            self.protect(ciphertext)
        }
    }

    #[derive(Debug)]
    struct FailingUnprotectProtector;

    impl SecretProtector for FailingUnprotectProtector {
        fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            Ok(plaintext.to_vec())
        }

        fn unprotect(&self, _: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            Err(InfrastructureError::KeychainUnprotect)
        }
    }

    fn shared_client_pkcs12() -> (Vec<u8>, Vec<u8>) {
        let certificate_service = CertificateService;
        let client_root = certificate_service
            .generate_root_ca("Shared Client Root")
            .expect("client root");
        let client_root_key = KeyPair::from_pkcs8_der_and_sign_algo(
            &client_root.private_key_pkcs8_der.as_slice().into(),
            &PKCS_ECDSA_P256_SHA256,
        )
        .expect("client root key");
        let client_issuer = Issuer::from_ca_cert_der(
            &client_root.certificate_der.as_slice().into(),
            client_root_key,
        )
        .expect("client issuer");
        let client_key =
            KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("client identity key");
        let mut client_params = CertificateParams::default();
        let mut client_name = DistinguishedName::new();
        client_name.push(DnType::CommonName, "Shared Client");
        client_params.distinguished_name = client_name;
        client_params.is_ca = IsCa::ExplicitNoCa;
        client_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client_certificate = client_params
            .signed_by(&client_key, &client_issuer)
            .expect("client certificate");
        let client_private_key = client_key.serialize_der();
        let mut keystore = KeyStore::new();
        keystore.add_entry(
            "shared",
            KeyStoreEntry::PrivateKeyChain(PrivateKeyChain::new(
                "shared-key",
                PrivateKey::from_der(&client_private_key).expect("client key"),
                [
                    Certificate::from_der(client_certificate.der()).expect("client x509"),
                    Certificate::from_der(&client_root.certificate_der).expect("client CA"),
                ],
            )),
        );
        let pkcs12 = keystore.writer("password").write().expect("pkcs12");
        (pkcs12, client_private_key)
    }

    fn assert_raw_pkcs12_secrets_are_not_persisted(store: &SqliteStore) {
        let protected = store
            .load_certificate_material(PKCS12)
            .expect("load protected PKCS12 material")
            .expect("PKCS12 material");
        let plaintext = protected
            .protected_blob
            .iter()
            .map(|byte| byte ^ 0xA5)
            .collect::<Vec<_>>();
        let persisted: serde_json::Value =
            serde_json::from_slice(&plaintext).expect("protected material JSON");
        assert!(persisted.get("password").is_none());
        assert!(persisted.get("pkcs12_der").is_none());
    }

    #[tokio::test]
    async fn tls_snapshot_preserves_keychain_unprotect_error_code() {
        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        store
            .compare_and_swap_certificate_materials(
                0,
                &[CertificateMaterialRecord {
                    kind: LEAF.into(),
                    protected_blob: vec![1],
                    metadata: serde_json::json!({"revision": 1}),
                    updated_at: Utc::now(),
                }],
            )
            .expect("seed protected material");
        let adapter = CertificateServiceAdapter::new(
            store,
            Arc::new(FailingUnprotectProtector),
            Arc::new(QueueDialog {
                open: ParkingMutex::new(VecDeque::new()),
            }),
            payment_profile(),
        );

        let error = adapter
            .load_epoch_snapshot(&["127.0.0.1".into()])
            .await
            .expect_err("snapshot must fail");

        assert_eq!(error.code, "KEYCHAIN_UNPROTECT_FAILED");
    }

    #[tokio::test]
    async fn tls_snapshot_distinguishes_corrupt_material_from_missing_material() {
        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        store
            .compare_and_swap_certificate_materials(
                0,
                &[CertificateMaterialRecord {
                    kind: LEAF.into(),
                    protected_blob: b"not-json".iter().map(|byte| byte ^ 0xA5).collect(),
                    metadata: serde_json::json!({"revision": 1}),
                    updated_at: Utc::now(),
                }],
            )
            .expect("seed corrupt protected material");
        let dialog = Arc::new(QueueDialog {
            open: ParkingMutex::new(VecDeque::new()),
        });
        let corrupt = CertificateServiceAdapter::new(
            store,
            Arc::new(XorProtector),
            dialog.clone(),
            payment_profile(),
        );
        let corrupt_error = corrupt
            .load_epoch_snapshot(&["127.0.0.1".into()])
            .await
            .expect_err("corrupt material must fail");
        assert_eq!(corrupt_error.code, "INTERNAL_ERROR");

        let missing = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(XorProtector),
            dialog,
            payment_profile(),
        );
        let missing_error = missing
            .load_epoch_snapshot(&["127.0.0.1".into()])
            .await
            .expect_err("missing material must fail");
        assert_eq!(missing_error.code, "CERTIFICATE_NOT_READY");
    }

    #[tokio::test]
    async fn certificate_imports_enforce_per_type_size_limits() {
        let directory = tempfile::tempdir().expect("tempdir");
        let pkcs12_path = directory.path().join("oversized.p12");
        std::fs::File::create(&pkcs12_path)
            .expect("create PKCS12")
            .set_len(PKCS12_IMPORT_MAX_BYTES + 1)
            .expect("size PKCS12");
        let ca_path = directory.path().join("oversized-ca.crt");
        std::fs::File::create(&ca_path)
            .expect("create CA")
            .set_len(CA_IMPORT_MAX_BYTES + 1)
            .expect("size CA");
        let adapter = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(XorProtector),
            Arc::new(QueueDialog {
                open: ParkingMutex::new(VecDeque::from([pkcs12_path, ca_path])),
            }),
            Arc::new(gmofg_proxy_product_payment::PaymentProductProfile::isolated_test_tool()),
        );

        let pkcs12_error = adapter
            .import_pkcs12("password".into())
            .await
            .expect_err("oversized PKCS12");
        assert_eq!(pkcs12_error.view_model.code, "IMPORT_TOO_LARGE");

        let ca_error = adapter
            .import_upstream_ca()
            .await
            .expect_err("oversized CA");
        assert_eq!(ca_error.view_model.code, "IMPORT_TOO_LARGE");
    }

    #[tokio::test]
    async fn export_ca_writes_only_the_bundled_public_pem_before_initialization() {
        let directory = tempfile::tempdir().expect("tempdir");
        let export_path = directory.path().join("gmofg-test-proxy-root-ca.crt");
        let adapter = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(XorProtector),
            Arc::new(ExportDialog {
                selection: ParkingMutex::new(Some(FileSelection {
                    path: export_path.clone(),
                    overwrite_confirmed: false,
                })),
            }),
            payment_profile(),
        );

        let result = adapter.export_ca().await.expect("export public Root CA");
        let exported = std::fs::read(&export_path).expect("read exported certificate");
        let expected = payment_profile().certificates().public_root_ca_pem();

        assert!(result.success);
        assert!(!result.cancelled);
        assert_eq!(exported, expected);
        assert!(exported.starts_with(b"-----BEGIN CERTIFICATE-----"));
        assert!(
            !exported
                .windows(b"PRIVATE KEY".len())
                .any(|part| part == b"PRIVATE KEY")
        );
        CertificateService
            .parse_ca(&exported)
            .expect("exported public certificate must parse as a CA");
        assert!(
            adapter
                .load_snapshot(&MATERIAL_KINDS)
                .expect("certificate snapshot")
                .materials
                .is_empty(),
            "export must not initialize or persist private material"
        );
    }

    #[tokio::test]
    async fn disabled_embedded_signing_still_allows_public_export_but_blocks_generation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let export_path = directory.path().join("public-root-ca.crt");
        let adapter = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(XorProtector),
            Arc::new(ExportDialog {
                selection: ParkingMutex::new(Some(FileSelection {
                    path: export_path.clone(),
                    overwrite_confirmed: false,
                })),
            }),
            Arc::new(PaymentProductProfile::default()),
        );

        adapter.export_ca().await.expect("public-only export");
        let exported = std::fs::read(export_path).expect("exported public certificate");
        assert!(exported.starts_with(b"-----BEGIN CERTIFICATE-----"));
        assert!(!exported.windows(11).any(|window| window == b"PRIVATE KEY"));

        let error = adapter
            .generate_ca(vec!["127.0.0.1".into()])
            .await
            .expect_err("test signing must fail closed");
        assert_eq!(error.view_model.code, "CERTIFICATE_TEST_SIGNING_DISABLED");
    }

    // CERT-005~017, SECURITY-006~009, TEST-TLS
    #[tokio::test]
    async fn protected_material_builds_a_complete_epoch_snapshot() {
        let directory = tempfile::tempdir().expect("tempdir");
        let certificate_service = CertificateService;
        let (pkcs12, client_private_key) = shared_client_pkcs12();
        let pkcs12_path = directory.path().join("shared.p12");
        std::fs::write(&pkcs12_path, pkcs12).expect("write pkcs12");

        let upstream = certificate_service
            .generate_root_ca("Upstream CA")
            .expect("upstream");
        let upstream_path = directory.path().join("upstream.cer");
        std::fs::write(&upstream_path, &upstream.certificate_der).expect("write upstream");

        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        let adapter = CertificateServiceAdapter::new(
            store.clone(),
            Arc::new(XorProtector),
            Arc::new(QueueDialog {
                open: ParkingMutex::new(VecDeque::from([pkcs12_path, upstream_path])),
            }),
            payment_profile(),
        );
        let generated = adapter
            .generate_ca(vec!["127.0.0.1".into()])
            .await
            .expect("generate");
        let duplicate = adapter
            .generate_ca(vec!["127.0.0.1".into()])
            .await
            .expect_err("duplicate generation must fail");
        assert_eq!(duplicate.view_model.code, "CERTIFICATE_ALREADY_EXISTS");
        assert!(duplicate.view_model.message.contains("重签服务端证书"));
        let bundled_overview = adapter
            .import_pkcs12("password".into())
            .await
            .expect("import pkcs12");
        assert_raw_pkcs12_secrets_are_not_persisted(&store);
        assert!(bundled_overview.ready);
        assert!(
            bundled_overview
                .items
                .iter()
                .any(|item| { item.usage.contains("内置 Payment server.crt") })
        );
        assert!(
            adapter
                .load_snapshot(&[UPSTREAM_CA])
                .expect("stored override snapshot")
                .materials
                .is_empty(),
            "bundled upstream CA must be a fallback, not a persisted override"
        );

        let overview = adapter.import_upstream_ca().await.expect("import upstream");
        assert!(
            overview.ready,
            "{:?}",
            adapter.validate().await.expect("validation")
        );
        assert!(
            overview
                .items
                .iter()
                .all(|item| item.valid_from.is_some() && item.valid_until.is_some())
        );
        assert!(overview.revision > generated.revision);
        assert!(
            overview
                .items
                .iter()
                .any(|item| item.usage.contains("用户替换"))
        );
        assert!(adapter.validate().await.expect("validate").valid);

        let snapshot = adapter
            .load_epoch_snapshot(&["127.0.0.1".into()])
            .await
            .expect("snapshot");
        assert_eq!(snapshot.upstream_client_certificate_chain_der.len(), 2);
        assert!(!snapshot.upstream_client_private_key_pkcs8_der.is_empty());
        let debug = format!("{adapter:?}");
        assert!(!debug.contains("password"));
        assert!(!debug.contains("PRIVATE"));

        let mut material_snapshot = adapter
            .load_snapshot(&MATERIAL_KINDS)
            .expect("load materials");
        material_snapshot
            .materials
            .get_mut(LEAF)
            .expect("leaf")
            .private_key_der = client_private_key;
        adapter
            .commit_snapshot(material_snapshot)
            .expect("replace leaf");
        assert!(!adapter.overview().await.expect("overview").ready);
        assert!(!adapter.validate().await.expect("validate").valid);
    }

    #[tokio::test]
    async fn separate_proxy_installations_share_only_the_test_root() {
        let dialog = || {
            Arc::new(QueueDialog {
                open: ParkingMutex::new(VecDeque::new()),
            })
        };
        let first = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("first store")),
            Arc::new(XorProtector),
            dialog(),
            payment_profile(),
        );
        let second = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("second store")),
            Arc::new(XorProtector),
            dialog(),
            payment_profile(),
        );

        first
            .generate_ca(vec!["10.0.34.50".into()])
            .await
            .expect("first proxy certificates");
        second
            .generate_ca(vec!["10.0.28.99".into()])
            .await
            .expect("second proxy certificates");

        let first_materials = first.load_snapshot(&[ROOT, LEAF]).expect("first snapshot");
        let second_materials = second
            .load_snapshot(&[ROOT, LEAF])
            .expect("second snapshot");
        let first_root = first_materials.materials.get(ROOT).expect("first root");
        let second_root = second_materials.materials.get(ROOT).expect("second root");
        let first_leaf = first_materials.materials.get(LEAF).expect("first leaf");
        let second_leaf = second_materials.materials.get(LEAF).expect("second leaf");

        assert_eq!(first_root.certificate_der, second_root.certificate_der);
        assert_eq!(first_root.fingerprint, second_root.fingerprint);
        assert_ne!(first_leaf.certificate_der, second_leaf.certificate_der);
        assert_ne!(first_leaf.private_key_der, second_leaf.private_key_der);
        assert_eq!(first_leaf.sans, vec!["IP:10.0.34.50"]);
        assert_eq!(second_leaf.sans, vec!["IP:10.0.28.99"]);
    }
}
