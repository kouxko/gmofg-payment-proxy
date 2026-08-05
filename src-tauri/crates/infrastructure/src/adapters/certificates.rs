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

impl InstallationServerIdentityProvider for CertificateServiceAdapter {
    fn load_installation_server_identity(&self) -> AppResult<ReverseClientIdentity> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&[ROOT, LEAF])?;
        let root = snapshot
            .materials
            .get(ROOT)
            .ok_or_else(|| AppError::new("CERTIFICATE_NOT_READY", "本机 Root CA 尚未初始化。"))?;
        let leaf = snapshot
            .materials
            .get(LEAF)
            .ok_or_else(|| AppError::new("CERTIFICATE_NOT_READY", "本机叶子证书尚未签发。"))?;
        let expected_sans = leaf
            .sans
            .iter()
            .map(|san| {
                san.trim_start_matches("DNS:")
                    .trim_start_matches("IP:")
                    .to_owned()
            })
            .collect::<Vec<_>>();
        self.certificates
            .validate_root(&root.certificate_der, &root.private_key_der)
            .and_then(|_| {
                self.certificates.validate_leaf(
                    &root.certificate_der,
                    &leaf.certificate_der,
                    &leaf.private_key_der,
                    &expected_sans,
                )
            })
            .map_err(app_error)?;
        Ok(ReverseClientIdentity {
            certificate_chain_der: vec![leaf.certificate_der.clone()],
            private_key_pkcs8_der: Zeroizing::new(leaf.private_key_der.clone()),
        })
    }
}

#[async_trait]
impl CertificateServicePort for CertificateServiceAdapter {
    async fn status(&self) -> AppResult<CertificateOverviewViewModel> {
        let _guard = self.material_lock.lock();
        self.status_locked()
    }

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
        let snapshot = self.load_snapshot(&[ROOT])?;
        let root = snapshot.materials.get(ROOT).ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_NOT_INITIALIZED",
                "当前安装实例尚未生成 Root CA，请先初始化证书。",
            )
        })?;
        let Some(selection) = self.dialog.choose_save_file("root_ca")? else {
            return Ok(cancelled(labels.export_cancelled_message));
        };
        infra(self.exporter.write(
            &selection.path,
            &certificate_der_to_pem(&root.certificate_der),
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
        let root = snapshot.materials.get(ROOT).cloned().ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_NOT_INITIALIZED",
                "当前安装实例尚未生成 Root CA。",
            )
        })?;
        self.certificates
            .validate_root(&root.certificate_der, &root.private_key_der)
            .map_err(app_error)?;
        let request = leaf_request(&sans)?;
        let leaf = self
            .certificates
            .generate_leaf(&root.certificate_der, &root.private_key_der, &request)
            .map_err(app_error)?;
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

/// 将存储中的 DER Root CA 转为标准 PEM。每 64 个 Base64 字符换行，便于 Android、
/// 浏览器和命令行工具直接导入；这里只编码公开证书，绝不会接触或导出私钥。
fn certificate_der_to_pem(der: &[u8]) -> Vec<u8> {
    let encoded = STANDARD.encode(der);
    let mut pem = Vec::with_capacity(encoded.len() + 64);
    pem.extend_from_slice(b"-----BEGIN CERTIFICATE-----\n");
    for line in encoded.as_bytes().chunks(64) {
        pem.extend_from_slice(line);
        pem.push(b'\n');
    }
    pem.extend_from_slice(b"-----END CERTIFICATE-----\n");
    pem
}

#[async_trait]
impl TlsMaterialProvider for CertificateServiceAdapter {
    async fn load_epoch_snapshot(
        &self,
        leaf_sans: &[String],
    ) -> intercept_proxy_runtime::Result<TlsMaterialSnapshot> {
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
                "installation Root CA missing",
            )
        })?;
        self.certificates
            .validate_root(&root.certificate_der, &root.private_key_der)
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

impl MitmCertificateAuthority for CertificateServiceAdapter {
    fn issue_server_identity(
        &self,
        authority_host: &str,
    ) -> intercept_proxy_runtime::Result<MitmServerIdentity> {
        let _guard = self.material_lock.lock();
        let snapshot = self.load_snapshot(&[ROOT]).map_err(proxy_app_error)?;
        let root = snapshot.materials.get(ROOT).ok_or_else(|| {
            ProxyError::new(
                ProxyErrorCode::CertificateNotReady,
                "installation Root CA missing",
            )
        })?;
        self.certificates
            .validate_root(&root.certificate_der, &root.private_key_der)
            .map_err(proxy_infra_error)?;
        let parsed_ip = authority_host.parse::<IpAddr>().ok();
        let request = LeafCertificateRequest {
            common_name: authority_host.to_owned(),
            dns_names: if parsed_ip.is_none() {
                vec![authority_host.to_owned()]
            } else {
                Vec::new()
            },
            ip_addresses: parsed_ip.into_iter().collect(),
        };
        let leaf = self
            .certificates
            .generate_leaf(&root.certificate_der, &root.private_key_der, &request)
            .map_err(proxy_infra_error)?;
        Ok(MitmServerIdentity {
            certificate_chain_der: vec![leaf.certificate_der.clone()],
            private_key_pkcs8_der: leaf.private_key_pkcs8_der.clone(),
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

fn item(kind: &str, usage: &str, material: &ProtectedMaterial) -> CertificateItemViewModel {
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

fn status_item(kind: &str, usage: &str, status: &MaterialStatus) -> CertificateItemViewModel {
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

fn parse_fingerprint(value: &str) -> intercept_proxy_runtime::Result<Vec<u8>> {
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
    use intercept_proxy_product_api::InterceptProxyProfile;

    fn test_profile() -> Arc<dyn ProductProfile> {
        Arc::new(InterceptProxyProfile)
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
            test_profile(),
        );

        let error = adapter
            .load_epoch_snapshot(&["127.0.0.1".into()])
            .await
            .expect_err("snapshot must fail");

        assert_eq!(error.code, "KEYCHAIN_UNPROTECT_FAILED");
    }

    #[tokio::test]
    async fn certificate_status_never_decrypts_private_material() {
        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        let metadata = |revision: u64, subject: &str| {
            serde_json::json!({
                "revision": revision,
                "subject": subject,
                "fingerprint": "AA:BB:CC",
                "sans": ["IP:127.0.0.1"],
                "not_before": "Jan  1 00:00:00 2026 GMT",
                "not_after": "Jan  1 00:00:00 2036 GMT"
            })
        };
        store
            .compare_and_swap_certificate_materials(
                0,
                &[
                    CertificateMaterialRecord {
                        kind: ROOT.into(),
                        protected_blob: vec![1],
                        metadata: metadata(1, "Test Root"),
                        updated_at: Utc::now(),
                    },
                    CertificateMaterialRecord {
                        kind: LEAF.into(),
                        protected_blob: vec![2],
                        metadata: metadata(1, "Test Leaf"),
                        updated_at: Utc::now(),
                    },
                ],
            )
            .expect("seed protected material");
        let adapter = CertificateServiceAdapter::new(
            store,
            Arc::new(FailingUnprotectProtector),
            Arc::new(QueueDialog {
                open: ParkingMutex::new(VecDeque::new()),
            }),
            Arc::new(InterceptProxyProfile),
        );

        let status = adapter.status().await.expect("metadata-only status");

        assert!(!status.can_initialize);
        assert!(status.ready);
        assert_eq!(status.status_text, "证书已就绪");
        assert_eq!(status.items.len(), 2);
        assert_eq!(status.items[0].kind, ROOT);
        assert_eq!(status.items[1].kind, LEAF);
        assert_eq!(status.items[0].subject, "Test Root");
        assert_eq!(status.items[1].subject, "Test Leaf");
        assert_eq!(
            adapter
                .overview()
                .await
                .expect_err("full overview still decrypts")
                .view_model
                .code,
            "KEYCHAIN_UNPROTECT_FAILED"
        );
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
            test_profile(),
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
            test_profile(),
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
            Arc::new(InterceptProxyProfile),
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
    async fn export_ca_writes_only_the_generated_public_pem() {
        let directory = tempfile::tempdir().expect("tempdir");
        let export_path = directory.path().join("intercept-proxy-root-ca.crt");
        let adapter = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(XorProtector),
            Arc::new(ExportDialog {
                selection: ParkingMutex::new(Some(FileSelection {
                    path: export_path.clone(),
                    overwrite_confirmed: false,
                })),
            }),
            test_profile(),
        );

        let overview = adapter
            .generate_ca(vec!["127.0.0.1".into()])
            .await
            .expect("generate installation Root CA");
        assert_eq!(overview.items.len(), 2);
        assert_eq!(overview.items[0].kind, ROOT);
        assert_eq!(overview.items[1].kind, LEAF);
        assert!(overview.items.iter().all(|item| {
            !item.subject.is_empty()
                && !item.sha256_fingerprint.is_empty()
                && item.valid_from.is_some()
                && item.valid_until.is_some()
        }));
        let result = adapter.export_ca().await.expect("export public Root CA");
        let exported = std::fs::read(&export_path).expect("read exported certificate");

        assert!(result.success);
        assert!(!result.cancelled);
        assert!(exported.starts_with(b"-----BEGIN CERTIFICATE-----"));
        assert!(
            !exported
                .windows(b"PRIVATE KEY".len())
                .any(|part| part == b"PRIVATE KEY")
        );
        CertificateService
            .parse_ca(&exported)
            .expect("exported public certificate must parse as a CA");
        assert_eq!(
            adapter
                .load_snapshot(&MATERIAL_KINDS)
                .expect("certificate snapshot")
                .materials
                .len(),
            2,
            "export must not add material beyond the generated Root and leaf"
        );
    }

    #[tokio::test]
    async fn listener_can_load_certificate_page_leaf_as_server_identity() {
        let adapter = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(XorProtector),
            Arc::new(QueueDialog {
                open: ParkingMutex::new(VecDeque::new()),
            }),
            test_profile(),
        );
        adapter
            .generate_ca(vec!["10.0.34.50".into()])
            .await
            .expect("generate certificate page materials");

        let identity = adapter
            .load_installation_server_identity()
            .expect("load installation leaf");

        assert_eq!(identity.certificate_chain_der.len(), 1);
        assert!(!identity.certificate_chain_der[0].is_empty());
        assert!(!identity.private_key_pkcs8_der.is_empty());
    }

    #[tokio::test]
    async fn mitm_signer_uses_the_protected_installation_root_for_each_authority() {
        let adapter = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(XorProtector),
            Arc::new(QueueDialog {
                open: ParkingMutex::new(VecDeque::new()),
            }),
            Arc::new(InterceptProxyProfile),
        );
        adapter
            .generate_ca(vec!["127.0.0.1".into()])
            .await
            .expect("generate installation Root CA");
        let identity = adapter
            .issue_server_identity("api.example.test")
            .expect("issue dynamic MITM leaf");
        let snapshot = adapter.load_snapshot(&[ROOT]).expect("load protected Root");
        let root = snapshot.materials.get(ROOT).expect("Root material");
        let metadata = CertificateService
            .validate_leaf(
                &root.certificate_der,
                &identity.certificate_chain_der[0],
                &identity.private_key_pkcs8_der,
                &["api.example.test".into()],
            )
            .expect("dynamic leaf must chain to installation Root");
        assert_eq!(metadata.san, vec!["DNS:api.example.test"]);
    }

    #[tokio::test]
    async fn generic_profile_generates_and_exports_per_installation_root() {
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
            Arc::new(InterceptProxyProfile),
        );

        adapter
            .generate_ca(vec!["127.0.0.1".into()])
            .await
            .expect("generic signing is generated at runtime");
        adapter.export_ca().await.expect("public-only export");
        let exported = std::fs::read(export_path).expect("exported public certificate");
        assert!(exported.starts_with(b"-----BEGIN CERTIFICATE-----"));
        assert!(!exported.windows(11).any(|window| window == b"PRIVATE KEY"));
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
            test_profile(),
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
        assert!(duplicate.view_model.message.contains("已经存在 Root CA"));
        let identity_overview = adapter
            .import_pkcs12("password".into())
            .await
            .expect("import pkcs12");
        assert_raw_pkcs12_secrets_are_not_persisted(&store);
        assert!(identity_overview.ready);
        assert_eq!(identity_overview.items.len(), 3);
        assert!(
            adapter
                .load_snapshot(&[UPSTREAM_CA])
                .expect("stored override snapshot")
                .materials
                .is_empty(),
            "未配置的上游 CA 不应被伪造为持久化材料"
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
                .any(|item| item.usage.contains("反向监听器导入"))
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
    async fn separate_proxy_installations_have_distinct_roots_and_leaves() {
        let dialog = || {
            Arc::new(QueueDialog {
                open: ParkingMutex::new(VecDeque::new()),
            })
        };
        let first = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("first store")),
            Arc::new(XorProtector),
            dialog(),
            test_profile(),
        );
        let second = CertificateServiceAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("second store")),
            Arc::new(XorProtector),
            dialog(),
            test_profile(),
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

        assert_ne!(first_root.certificate_der, second_root.certificate_der);
        assert_ne!(first_root.fingerprint, second_root.fingerprint);
        assert_ne!(first_leaf.certificate_der, second_leaf.certificate_der);
        assert_ne!(first_leaf.private_key_der, second_leaf.private_key_der);
        assert_eq!(first_leaf.sans, vec!["IP:10.0.34.50"]);
        assert_eq!(second_leaf.sans, vec!["IP:10.0.28.99"]);
    }
}
