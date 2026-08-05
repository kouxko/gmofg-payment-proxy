//! 证书应用服务适配器：连接持久化密文、系统密钥保护器与证书解析器。
//!
//! 私钥只在导入、签发或构建 TLS 快照时短暂解密；数据库保存的是受当前用户保护的密文。
//! 任一步失败都不发布部分证书快照，避免证书与私钥来自不同版本。

use std::{collections::BTreeMap, fmt, net::IpAddr, sync::Arc};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use intercept_proxy_application::{
    AppError, AppResult, CertificateItemViewModel, CertificateOverviewViewModel,
    CertificateServicePort, CertificateValidationViewModel, FieldValidationViewModel,
    OperationResultViewModel, UiTone,
};
use intercept_proxy_product_api::{ProductCertificatePolicy, ProductProfile};
use intercept_proxy_runtime::{
    ErrorCode as ProxyErrorCode, MitmCertificateAuthority, MitmServerIdentity, ProxyError,
    ReverseClientIdentity, TlsMaterialProvider, TlsMaterialSnapshot,
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
    listener_runtime::InstallationServerIdentityProvider,
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

/// 可直接用于状态栏和列表的非敏感证书元数据。
///
/// 它刻意不包含证书 DER、私钥或保护后的密文，因此读取该结构不需要访问系统密钥库。
#[derive(Debug, Clone, Deserialize)]
struct MaterialStatus {
    revision: u64,
    subject: String,
    fingerprint: String,
    #[serde(default)]
    sans: Vec<String>,
    #[serde(default)]
    not_before: Option<String>,
    #[serde(default)]
    not_after: Option<String>,
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

    /// 读取不会触发 Keychain/DPAPI 的证书状态快照。
    ///
    /// 完整证书校验仍由 `overview_locked`/`validate` 执行；启动快照只需要知道材料是否
    /// 已配置以及它们公开的主题、指纹和 SAN。
    fn status_locked(&self) -> AppResult<CertificateOverviewViewModel> {
        let labels = self.certificate_policy().labels();
        let snapshot = infra(
            self.store
                .load_certificate_materials_snapshot(&MATERIAL_KINDS),
        )?;
        let mut statuses = BTreeMap::new();
        for record in snapshot.records {
            let status: MaterialStatus = serde_json::from_value(record.metadata)
                .map_err(|error| json_error("证书元数据无效", error))?;
            if status.revision != snapshot.revision {
                return Err(AppError::new(
                    "CERTIFICATE_INVALID",
                    "证书元数据修订号与聚合修订号不一致。",
                ));
            }
            statuses.insert(record.kind, status);
        }

        let upstream_is_override = statuses.contains_key(UPSTREAM_CA);
        let bundled_upstream = if upstream_is_override {
            None
        } else {
            self.bundled_upstream_material(snapshot.revision)?
        };
        let configured = |kind: &str| {
            statuses.contains_key(kind) || (kind == UPSTREAM_CA && bundled_upstream.is_some())
        };
        // 全局状态只表示当前安装实例的下游签发链是否可用。上游 PKCS12 和 CA
        // 都是按入口选择的可选材料，不能因为某个未使用的可选槽位为空就把顶部栏
        // 误报成“尚未初始化”。具体入口启动前仍会校验它实际引用的全部材料。
        let policy = self.certificate_policy();
        let ready = [ROOT, LEAF].into_iter().all(configured)
            && (!policy.requires_global_client_identity() || configured(PKCS12))
            && (!policy.requires_global_upstream_ca() || configured(UPSTREAM_CA));

        let mut items = Vec::new();
        for (kind, _display, usage) in [
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
        ] {
            if let Some(status) = statuses.get(kind) {
                items.push(status_item(kind, usage, status));
            } else if kind == UPSTREAM_CA
                && let Some(material) = bundled_upstream.as_ref()
            {
                items.push(item(kind, usage, material));
            }
        }

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
            items,
            can_initialize: !statuses.contains_key(ROOT) && !statuses.contains_key(LEAF),
            can_change: true,
            disabled_reason: None,
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
                "not_before": material.not_before,
                "not_after": material.not_after,
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
            .generate_root_ca(self.certificate_policy().labels().root_name)
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
                .filter_map(|(kind, _, usage, material)| {
                    material
                        .as_ref()
                        .map(|material| item(kind, usage, material))
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
        // Root 与叶子证书组成当前安装实例的基础签发链。PKCS12 和上游 CA
        // 只有在具体入口引用时才是必需项；这里仅在它们已导入时校验内容。
        let policy = self.certificate_policy();
        let required = [
            (ROOT, true),
            (LEAF, true),
            (PKCS12, policy.requires_global_client_identity()),
            (UPSTREAM_CA, policy.requires_global_upstream_ca()),
        ];
        for (kind, is_required) in required {
            if !is_required {
                continue;
            }
            if find(kind).is_none() {
                errors.insert(kind.into(), vec!["证书材料尚未配置。".into()]);
            }
        }
        let root_metadata = find(ROOT).and_then(|root| {
            match self
                .certificates
                .validate_root(&root.certificate_der, &root.private_key_der)
            {
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

mod helpers;
mod ports;

use helpers::{
    from_bundle, item, leaf_request, material_matches, parse_fingerprint, proxy_app_error,
    proxy_infra_error, status_item, verify_revision,
};

#[cfg(test)]
#[path = "certificates_tests.rs"]
mod certificates_tests;
