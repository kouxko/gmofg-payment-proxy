//! 每个 Listener 独立的上游 TLS 材料导入与安全解析。

use std::{fmt, sync::Arc};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, CertificateItemViewModel, CertificateReference, CertificateReferenceKind,
    ListenerCertificateImportPort, ListenerCertificateImportViewModel, PortableCertificateMaterial,
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[cfg(test)]
use crate::SqliteStore;
use crate::{
    CertificateService, IntoSqlitePersistence, ProtectedSecretRecord, SecretProtector,
    SqliteExecutor,
};

use super::{
    NativeFileDialog,
    common::{app_error, infra},
    listener_certificate_metadata::{inspect_file_reference, view_model},
    listener_certificate_store::{
        FORMAT_VERSION, ManagedMaterial, REFERENCE_PREFIX, decode_material, managed_key,
        read_secret_file,
    },
};

#[path = "listener_certificate_portable.rs"]
mod portable;
use portable::validate_portable_material;

#[path = "listener_certificate_reference.rs"]
mod reference_support;
use reference_support::{ensure_kind_matches, imported, kind_mismatch, reference};

const PROVIDER: &str = "listener_tls";
const KIND_UPSTREAM_CLIENT_IDENTITY: u8 = 1;
const KIND_UPSTREAM_SERVER_TRUST: u8 = 2;
const KIND_DOWNSTREAM_SERVER_IDENTITY: u8 = 3;
const KIND_DOWNSTREAM_CLIENT_TRUST: u8 = 4;
const KIND_UPSTREAM_CLIENT_IDENTITY_PEM: u8 = 5;
const MAX_PORTABLE_MATERIAL_BYTES: usize = 16 * 1024 * 1024;

pub struct ManagedListenerCertificateAdapter {
    executor: SqliteExecutor,
    protector: Arc<dyn SecretProtector>,
    dialog: Arc<dyn NativeFileDialog>,
}

impl fmt::Debug for ManagedListenerCertificateAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedListenerCertificateAdapter")
            .field("executor", &self.executor)
            .field("protector", &"<system secret protector>")
            .field("dialog", &self.dialog)
            .finish()
    }
}

impl ManagedListenerCertificateAdapter {
    #[must_use]
    pub fn new(
        persistence: impl IntoSqlitePersistence,
        protector: Arc<dyn SecretProtector>,
        dialog: Arc<dyn NativeFileDialog>,
    ) -> Self {
        let (executor, store) = persistence.into_sqlite_persistence();
        drop(store);
        Self {
            executor,
            protector,
            dialog,
        }
    }

    async fn persist_async(&self, kind: u8, password: &[u8], bytes: &[u8]) -> AppResult<String> {
        let (key, record) = self.protected_record(kind, password, bytes)?;
        self.executor
            .execute(move |store| store.save_protected_secret(&record).map_err(AppError::from))
            .await?;
        Ok(key)
    }

    fn protected_record(
        &self,
        kind: u8,
        password: &[u8],
        bytes: &[u8],
    ) -> AppResult<(String, ProtectedSecretRecord)> {
        let mut plaintext = Zeroizing::new(Vec::with_capacity(6 + password.len() + bytes.len()));
        plaintext.push(FORMAT_VERSION);
        plaintext.push(kind);
        let password_len = u32::try_from(password.len())
            .map_err(|_| AppError::new("CERTIFICATE_INVALID", "PKCS12 密码长度超出支持范围。"))?;
        plaintext.extend_from_slice(&password_len.to_be_bytes());
        plaintext.extend_from_slice(password);
        plaintext.extend_from_slice(bytes);
        let protected_blob = infra(self.protector.protect(&plaintext))?;
        let key = Uuid::new_v4().to_string();
        let record = ProtectedSecretRecord {
            provider: PROVIDER.into(),
            key: key.clone(),
            protected_blob,
            updated_at: Utc::now(),
        };
        Ok((key, record))
    }

    async fn load_async(&self, key: &str) -> AppResult<ManagedMaterial> {
        let key = key.to_owned();
        let record = self
            .executor
            .execute(move |store| {
                store
                    .load_protected_secret(PROVIDER, &key)
                    .map_err(AppError::from)
            })
            .await?
            .ok_or_else(|| {
                AppError::new(
                    "CERTIFICATE_NOT_READY",
                    "Listener TLS 安全引用不存在，请重新导入证书材料。",
                )
            })?;
        let plaintext = Zeroizing::new(infra(self.protector.unprotect(&record.protected_blob))?);
        decode_material(plaintext)
    }
}

mod resolution;

#[async_trait]
impl ListenerCertificateImportPort for ManagedListenerCertificateAdapter {
    async fn import_downstream_server_identity(
        &self,
        label: String,
        password: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        let password = Zeroizing::new(password);
        let Some(path) = self.dialog.choose_open_file("server_identity_pem")? else {
            return Ok(None);
        };
        let bytes = read_secret_file(&path)?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        let parsed = match extension.as_deref() {
            Some("p12" | "pfx") => CertificateService
                .parse_server_identity_pkcs12(&bytes, &password)
                .map_err(app_error)?,
            Some("pem") => CertificateService
                .parse_server_identity_pem(&bytes, &password)
                .map_err(app_error)?,
            _ => {
                return Err(AppError::new(
                    "CERTIFICATE_INVALID",
                    "下游服务端身份仅支持 .p12、.pfx 或包含证书链与私钥的 .pem。",
                ));
            }
        };
        let canonical_pem = parsed.canonical_pem();
        let key = self
            .persist_async(KIND_DOWNSTREAM_SERVER_IDENTITY, &[], &canonical_pem)
            .await?;
        let reference = reference(label, CertificateReferenceKind::ReverseServerIdentity, &key);
        Ok(Some(imported(
            reference,
            view_model(
                CertificateReferenceKind::ReverseServerIdentity,
                parsed.metadata.clone(),
            ),
        )))
    }

    async fn import_downstream_client_trust(
        &self,
        label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        let Some(path) = self.dialog.choose_open_file("downstream_client_ca")? else {
            return Ok(None);
        };
        let bytes = read_secret_file(&path)?;
        let trusted = CertificateService
            .parse_client_trust_anchor(&bytes)
            .map_err(app_error)?;
        let key = self
            .persist_async(KIND_DOWNSTREAM_CLIENT_TRUST, &[], &trusted.certificate_der)
            .await?;
        let reference = reference(label, CertificateReferenceKind::DownstreamClientTrust, &key);
        Ok(Some(imported(
            reference,
            view_model(
                CertificateReferenceKind::DownstreamClientTrust,
                trusted.metadata,
            ),
        )))
    }

    async fn import_upstream_client_identity(
        &self,
        label: String,
        password: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        let password = Zeroizing::new(password);
        let Some(path) = self.dialog.choose_open_file("upstream_client_identity")? else {
            return Ok(None);
        };
        let bytes = read_secret_file(&path)?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        let (stored_kind, stored_password, metadata) = match extension.as_deref() {
            Some("p12" | "pfx") => {
                let parsed = CertificateService
                    .parse_pkcs12(&bytes, &password)
                    .map_err(app_error)?;
                (
                    KIND_UPSTREAM_CLIENT_IDENTITY,
                    password.as_bytes(),
                    parsed.metadata.clone(),
                )
            }
            Some("pem") => {
                let parsed = CertificateService
                    .parse_client_identity_pem(&bytes)
                    .map_err(app_error)?;
                (
                    KIND_UPSTREAM_CLIENT_IDENTITY_PEM,
                    &[][..],
                    parsed.metadata.clone(),
                )
            }
            _ => {
                return Err(AppError::new(
                    "CERTIFICATE_INVALID",
                    "上游客户端身份仅支持 .p12、.pfx 或包含证书链与私钥的 .pem。",
                ));
            }
        };
        let key = self
            .persist_async(stored_kind, stored_password, &bytes)
            .await?;
        let reference = reference(
            label,
            CertificateReferenceKind::UpstreamClientIdentity,
            &key,
        );
        Ok(Some(imported(
            reference,
            view_model(CertificateReferenceKind::UpstreamClientIdentity, metadata),
        )))
    }

    async fn import_upstream_server_trust(
        &self,
        label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        let Some(path) = self.dialog.choose_open_file("upstream_ca")? else {
            return Ok(None);
        };
        let bytes = read_secret_file(&path)?;
        let trusted = CertificateService
            .parse_upstream_ca(&bytes)
            .map_err(app_error)?;
        let key = self
            .persist_async(KIND_UPSTREAM_SERVER_TRUST, &[], trusted.canonical_bytes())
            .await?;
        let reference = reference(label, CertificateReferenceKind::UpstreamServerTrust, &key);
        Ok(Some(imported(
            reference,
            view_model(
                CertificateReferenceKind::UpstreamServerTrust,
                trusted.metadata,
            ),
        )))
    }

    async fn inspect(
        &self,
        reference: CertificateReference,
    ) -> AppResult<CertificateItemViewModel> {
        let Some(key) = managed_key(&reference.reference) else {
            return inspect_file_reference(&reference);
        };
        let material = self.load_async(key?).await?;
        let metadata = match (reference.kind, material.kind) {
            (CertificateReferenceKind::UpstreamClientIdentity, KIND_UPSTREAM_CLIENT_IDENTITY) => {
                let password = std::str::from_utf8(&material.password).map_err(|_| {
                    AppError::new("CERTIFICATE_NOT_READY", "受保护的 PKCS12 密码编码无效。")
                })?;
                CertificateService
                    .parse_pkcs12(&material.bytes, password)
                    .map_err(app_error)?
                    .metadata
                    .clone()
            }
            (
                CertificateReferenceKind::UpstreamClientIdentity,
                KIND_UPSTREAM_CLIENT_IDENTITY_PEM,
            ) => CertificateService
                .parse_client_identity_pem(&material.bytes)
                .map_err(app_error)?
                .metadata
                .clone(),
            (CertificateReferenceKind::UpstreamServerTrust, KIND_UPSTREAM_SERVER_TRUST) => {
                CertificateService
                    .parse_upstream_ca(&material.bytes)
                    .map_err(app_error)?
                    .metadata
            }
            (CertificateReferenceKind::ReverseServerIdentity, KIND_DOWNSTREAM_SERVER_IDENTITY) => {
                CertificateService
                    .parse_server_identity_pem(&material.bytes, "")
                    .map_err(app_error)?
                    .metadata
                    .clone()
            }
            (CertificateReferenceKind::DownstreamClientTrust, KIND_DOWNSTREAM_CLIENT_TRUST) => {
                CertificateService
                    .parse_client_trust_anchor(&material.bytes)
                    .map_err(app_error)?
                    .metadata
            }
            _ => return Err(kind_mismatch()),
        };
        Ok(view_model(reference.kind, metadata))
    }

    async fn export_portable(
        &self,
        reference: CertificateReference,
    ) -> AppResult<PortableCertificateMaterial> {
        let key = managed_key(&reference.reference).ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_EXPORT_FORBIDDEN",
                "只能导出由 Intercept Proxy 托管的 Listener TLS 证书材料。",
            )
        })??;
        let material = self.load_async(key).await?;
        ensure_kind_matches(reference.kind, material.kind)?;
        // PKCS#12 的空密码与“不提供密码”含义不同。可移植文档必须保留 Some("")，
        // 否则导出后再导入会被 `validate_portable_material` 判定为缺少密码。
        let password = if material.kind == KIND_UPSTREAM_CLIENT_IDENTITY {
            Some(
                std::str::from_utf8(&material.password)
                    .map_err(|_| {
                        AppError::new("CERTIFICATE_NOT_READY", "受保护的 PKCS12 密码编码无效。")
                    })?
                    .to_owned(),
            )
        } else {
            None
        };
        Ok(PortableCertificateMaterial {
            reference_id: reference.id,
            label: reference.label,
            kind: reference.kind,
            material_base64: STANDARD.encode(&material.bytes),
            material_sha256: intercept_proxy_application::portable_material_sha256(&material.bytes),
            password,
        })
    }

    async fn restore_portable(
        &self,
        material: PortableCertificateMaterial,
    ) -> AppResult<CertificateReference> {
        let (stored_kind, password, stored_bytes) = decode_portable_material(&material)?;
        let key = self
            .persist_async(stored_kind, password.as_bytes(), &stored_bytes)
            .await?;
        Ok(CertificateReference {
            id: material.reference_id,
            label: material.label,
            kind: material.kind,
            reference: format!("{REFERENCE_PREFIX}{key}"),
        })
    }

    async fn preflight_portable(&self, material: &PortableCertificateMaterial) -> AppResult<()> {
        decode_portable_material(material).map(|_| ())
    }

    async fn application_backup_baseline(&self) -> AppResult<[u8; 32]> {
        self.executor
            .execute(|store| {
                store
                    .protected_secret_fingerprint(PROVIDER)
                    .map_err(AppError::from)
            })
            .await
    }

    async fn discard(&self, reference: CertificateReference) -> AppResult<()> {
        let key = managed_key(&reference.reference).ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_DISCARD_FORBIDDEN",
                "只能清理由 Intercept Proxy 托管且尚未保存的证书材料。",
            )
        })??;
        let material = self.load_async(key).await?;
        ensure_kind_matches(reference.kind, material.kind)?;
        let key = key.to_owned();
        let deleted = self
            .executor
            .execute(move |store| {
                store
                    .delete_protected_secret(PROVIDER, &key)
                    .map_err(AppError::from)
            })
            .await?;
        if !deleted {
            return Err(AppError::new(
                "CERTIFICATE_NOT_READY",
                "待清理的 Listener TLS 安全引用不存在。",
            ));
        }
        Ok(())
    }
}

fn decode_portable_material(
    material: &PortableCertificateMaterial,
) -> AppResult<(u8, String, Vec<u8>)> {
    material.validate_shape()?;
    let bytes = STANDARD.decode(&material.material_base64).map_err(|_| {
        AppError::new(
            "PORTABLE_CERTIFICATE_INVALID",
            "配置文件中的证书材料不是有效的 Base64。",
        )
        .entity(material.reference_id.to_string())
    })?;
    if bytes.is_empty() || bytes.len() > MAX_PORTABLE_MATERIAL_BYTES {
        return Err(AppError::new(
            "PORTABLE_CERTIFICATE_INVALID",
            "配置文件中的证书材料为空或超过 16 MiB 上限。",
        )
        .entity(material.reference_id.to_string()));
    }
    validate_portable_material(material, &bytes)
}

#[cfg(test)]
#[path = "listener_certificates_tests.rs"]
mod tests;
