//! 每个 Listener 独立的上游 TLS 材料导入与安全解析。

use std::{fmt, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, CertificateItemViewModel, CertificateReference, CertificateReferenceId,
    CertificateReferenceKind, ListenerCertificateDetailViewModel, ListenerCertificateImportPort,
    ListenerCertificateImportViewModel,
};
use intercept_proxy_runtime::ReverseClientIdentity;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{CertificateService, ProtectedSecretRecord, SecretProtector, SqliteStore};

use super::{
    NativeFileDialog,
    common::{app_error, infra},
    listener_certificate_metadata::{inspect_file_reference, view_model},
    listener_certificate_store::{
        FORMAT_VERSION, ManagedMaterial, REFERENCE_PREFIX, decode_material, managed_key,
        read_secret_file,
    },
};

const PROVIDER: &str = "listener_tls";
const KIND_UPSTREAM_CLIENT_IDENTITY: u8 = 1;
const KIND_UPSTREAM_SERVER_TRUST: u8 = 2;
const KIND_DOWNSTREAM_SERVER_IDENTITY: u8 = 3;
const KIND_DOWNSTREAM_CLIENT_TRUST: u8 = 4;

pub struct ManagedListenerCertificateAdapter {
    store: Arc<SqliteStore>,
    protector: Arc<dyn SecretProtector>,
    dialog: Arc<dyn NativeFileDialog>,
}

impl fmt::Debug for ManagedListenerCertificateAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManagedListenerCertificateAdapter")
            .field("store", &self.store)
            .field("protector", &"<system secret protector>")
            .field("dialog", &self.dialog)
            .finish()
    }
}

impl ManagedListenerCertificateAdapter {
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        protector: Arc<dyn SecretProtector>,
        dialog: Arc<dyn NativeFileDialog>,
    ) -> Self {
        Self {
            store,
            protector,
            dialog,
        }
    }

    pub fn resolve_trust(
        &self,
        reference: &CertificateReference,
    ) -> Option<AppResult<Vec<Vec<u8>>>> {
        let key = match managed_key(&reference.reference)? {
            Ok(key) => key,
            Err(error) => return Some(Err(error)),
        };
        Some(self.load(key).and_then(|material| {
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
            Ok(vec![trusted.certificate_der])
        }))
    }

    pub fn resolve_identity(
        &self,
        reference: &CertificateReference,
    ) -> Option<AppResult<ReverseClientIdentity>> {
        let key = match managed_key(&reference.reference)? {
            Ok(key) => key,
            Err(error) => return Some(Err(error)),
        };
        Some(
            self.load(key)
                .and_then(|material| match (reference.kind, material.kind) {
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
                            private_key_pkcs8_der: std::mem::take(
                                &mut parsed.private_key_pkcs8_der,
                            ),
                        })
                    }
                    (
                        CertificateReferenceKind::ReverseServerIdentity,
                        KIND_DOWNSTREAM_SERVER_IDENTITY,
                    ) => {
                        let mut parsed = CertificateService
                            .parse_server_identity_pem(&material.bytes)
                            .map_err(app_error)?;
                        Ok(ReverseClientIdentity {
                            certificate_chain_der: std::mem::take(
                                &mut parsed.certificate_chain_der,
                            ),
                            private_key_pkcs8_der: std::mem::take(
                                &mut parsed.private_key_pkcs8_der,
                            ),
                        })
                    }
                    _ => Err(kind_mismatch()),
                }),
        )
    }

    fn persist(&self, kind: u8, password: &[u8], bytes: &[u8]) -> AppResult<String> {
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
        infra(self.store.save_protected_secret(&ProtectedSecretRecord {
            provider: PROVIDER.into(),
            key: key.clone(),
            protected_blob,
            updated_at: Utc::now(),
        }))?;
        Ok(key)
    }

    fn load(&self, key: &str) -> AppResult<ManagedMaterial> {
        let record = infra(self.store.load_protected_secret(PROVIDER, key))?.ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_NOT_READY",
                "Listener TLS 安全引用不存在，请重新导入证书材料。",
            )
        })?;
        let plaintext = Zeroizing::new(infra(self.protector.unprotect(&record.protected_blob))?);
        decode_material(plaintext)
    }
}

#[async_trait]
impl ListenerCertificateImportPort for ManagedListenerCertificateAdapter {
    async fn import_downstream_server_identity(
        &self,
        label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        let Some(path) = self.dialog.choose_open_file("server_identity_pem")? else {
            return Ok(None);
        };
        let bytes = read_secret_file(&path)?;
        let parsed = CertificateService
            .parse_server_identity_pem(&bytes)
            .map_err(app_error)?;
        let key = self.persist(KIND_DOWNSTREAM_SERVER_IDENTITY, &[], &bytes)?;
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
        let key = self.persist(KIND_DOWNSTREAM_CLIENT_TRUST, &[], &trusted.certificate_der)?;
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
        let Some(path) = self.dialog.choose_open_file("pkcs12")? else {
            return Ok(None);
        };
        let bytes = read_secret_file(&path)?;
        let parsed = CertificateService
            .parse_pkcs12(&bytes, &password)
            .map_err(app_error)?;
        let key = self.persist(KIND_UPSTREAM_CLIENT_IDENTITY, password.as_bytes(), &bytes)?;
        let reference = reference(
            label,
            CertificateReferenceKind::UpstreamClientIdentity,
            &key,
        );
        Ok(Some(imported(
            reference,
            view_model(
                CertificateReferenceKind::UpstreamClientIdentity,
                parsed.metadata.clone(),
            ),
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
        let key = self.persist(KIND_UPSTREAM_SERVER_TRUST, &[], &trusted.certificate_der)?;
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
        let material = self.load(key?)?;
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
            (CertificateReferenceKind::UpstreamServerTrust, KIND_UPSTREAM_SERVER_TRUST) => {
                CertificateService
                    .parse_upstream_ca(&material.bytes)
                    .map_err(app_error)?
                    .metadata
            }
            (CertificateReferenceKind::ReverseServerIdentity, KIND_DOWNSTREAM_SERVER_IDENTITY) => {
                CertificateService
                    .parse_server_identity_pem(&material.bytes)
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

    async fn discard(&self, reference: CertificateReference) -> AppResult<()> {
        let key = managed_key(&reference.reference).ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_DISCARD_FORBIDDEN",
                "只能清理由 Intercept Proxy 托管且尚未保存的证书材料。",
            )
        })??;
        let material = self.load(key)?;
        let kind_matches = matches!(
            (reference.kind, material.kind),
            (
                CertificateReferenceKind::UpstreamClientIdentity,
                KIND_UPSTREAM_CLIENT_IDENTITY
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
        if !kind_matches {
            return Err(kind_mismatch());
        }
        let deleted = infra(self.store.delete_protected_secret(PROVIDER, key))?;
        if !deleted {
            return Err(AppError::new(
                "CERTIFICATE_NOT_READY",
                "待清理的 Listener TLS 安全引用不存在。",
            ));
        }
        Ok(())
    }
}

fn reference(label: String, kind: CertificateReferenceKind, key: &str) -> CertificateReference {
    CertificateReference {
        id: CertificateReferenceId::new(),
        label,
        kind,
        reference: format!("{REFERENCE_PREFIX}{key}"),
    }
}

fn imported(
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

fn kind_mismatch() -> AppError {
    AppError::new(
        "CERTIFICATE_NOT_READY",
        "Listener TLS 安全引用的材料类型不匹配。",
    )
}

#[cfg(test)]
#[path = "listener_certificates_tests.rs"]
mod tests;
