use std::{collections::VecDeque, path::PathBuf};

use intercept_proxy_application::CertificateReferenceId;
use parking_lot::Mutex;

use super::*;
use crate::{InfrastructureError, adapters::FileSelection};

#[path = "listener_certificates_test_support.rs"]
mod test_support;

use test_support::{
    client_identity_pem, client_pkcs12, client_pkcs12_with_password, server_identity_pem,
};

#[derive(Debug)]
struct QueueDialog(Mutex<VecDeque<PathBuf>>);

impl NativeFileDialog for QueueDialog {
    fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
        Ok(self.0.lock().pop_front())
    }

    fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(plaintext.iter().map(|byte| byte ^ 0xA5).collect())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(ciphertext.iter().map(|byte| byte ^ 0xA5).collect())
    }
}

#[tokio::test]
async fn imports_are_independent_protected_references_and_resolve_in_memory() {
    let directory = tempfile::tempdir().unwrap();
    let (pkcs12, private_key, ca_der) = client_pkcs12();
    let (server_pem, server_private_key, downstream_ca_der) = server_identity_pem();
    let downstream_identity = directory.path().join("downstream.pem");
    let downstream_trust = directory.path().join("downstream-client.crt");
    let first_identity = directory.path().join("first.p12");
    let second_identity = directory.path().join("second.p12");
    let first_trust = directory.path().join("first.crt");
    let second_trust = directory.path().join("second.crt");
    std::fs::write(&first_identity, &pkcs12).unwrap();
    std::fs::write(&second_identity, &pkcs12).unwrap();
    std::fs::write(&first_trust, &ca_der).unwrap();
    std::fs::write(&second_trust, &ca_der).unwrap();
    std::fs::write(&downstream_identity, &server_pem).unwrap();
    std::fs::write(&downstream_trust, &downstream_ca_der).unwrap();

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let adapter = ManagedListenerCertificateAdapter::new(
        store.clone(),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::from([
            downstream_identity,
            downstream_trust,
            first_identity,
            second_identity,
            first_trust,
            second_trust,
        ])))),
    );

    let downstream_identity = adapter
        .import_downstream_server_identity("入口服务端身份".into())
        .await
        .unwrap()
        .unwrap();
    let downstream_trust = adapter
        .import_downstream_client_trust("终端客户端 CA".into())
        .await
        .unwrap()
        .unwrap();

    let identity_a = adapter
        .import_upstream_client_identity("入口 A 身份".into(), "password".into())
        .await
        .unwrap()
        .unwrap();
    let identity_b = adapter
        .import_upstream_client_identity("入口 B 身份".into(), "password".into())
        .await
        .unwrap()
        .unwrap();
    let trust_a = adapter
        .import_upstream_server_trust("入口 A 信任".into())
        .await
        .unwrap()
        .unwrap();
    let trust_b = adapter
        .import_upstream_server_trust("入口 B 信任".into())
        .await
        .unwrap()
        .unwrap();

    assert_imported_certificate_subjects(&identity_a, &downstream_identity, &trust_a);
    let downstream_identity = downstream_identity.reference;
    let downstream_trust = downstream_trust.reference;
    let identity_a = identity_a.reference;
    let identity_b = identity_b.reference;
    let trust_a = trust_a.reference;
    let trust_b = trust_b.reference;
    assert_ne!(identity_a.id, identity_b.id);
    assert_ne!(identity_a.reference, identity_b.reference);
    assert_ne!(trust_a.reference, trust_b.reference);
    assert!(identity_a.reference.starts_with(REFERENCE_PREFIX));
    assert!(!identity_a.reference.contains("password"));
    assert!(!identity_a.reference.contains(".p12"));

    let references = [
        &downstream_identity,
        &downstream_trust,
        &identity_a,
        &identity_b,
        &trust_a,
        &trust_b,
    ];
    assert_references_are_protected(&store, &references, &pkcs12);
    assert_resolved_material(
        &adapter,
        &identity_a,
        &trust_b,
        &downstream_identity,
        &downstream_trust,
        &private_key,
        &ca_der,
        &server_private_key,
        &downstream_ca_der,
    );
    assert_inspected_details(&adapter, identity_a, trust_b).await;
}

#[tokio::test]
async fn discarded_managed_reference_removes_only_its_protected_material() {
    let directory = tempfile::tempdir().unwrap();
    let (pkcs12, _, _) = client_pkcs12();
    let first = directory.path().join("first.p12");
    let second = directory.path().join("second.p12");
    std::fs::write(&first, &pkcs12).unwrap();
    std::fs::write(&second, &pkcs12).unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let adapter = ManagedListenerCertificateAdapter::new(
        store.clone(),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::from([first, second])))),
    );
    let discarded = adapter
        .import_upstream_client_identity("放弃的身份".into(), "password".into())
        .await
        .unwrap()
        .unwrap()
        .reference;
    let retained = adapter
        .import_upstream_client_identity("保留的身份".into(), "password".into())
        .await
        .unwrap()
        .unwrap()
        .reference;
    let discarded_key = managed_key(&discarded.reference)
        .unwrap()
        .unwrap()
        .to_owned();
    let retained_key = managed_key(&retained.reference)
        .unwrap()
        .unwrap()
        .to_owned();

    adapter.discard(discarded).await.unwrap();

    assert!(
        store
            .load_protected_secret(PROVIDER, &discarded_key)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .load_protected_secret(PROVIDER, &retained_key)
            .unwrap()
            .is_some()
    );
    assert!(adapter.inspect(retained).await.is_ok());
}

#[tokio::test]
async fn discard_rejects_non_managed_references() {
    let adapter = ManagedListenerCertificateAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::new()))),
    );
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "外部引用".into(),
        kind: CertificateReferenceKind::UpstreamServerTrust,
        reference: "file:/tmp/root.crt".into(),
    };

    let error = adapter.discard(reference).await.unwrap_err();

    assert_eq!(error.view_model.code, "CERTIFICATE_DISCARD_FORBIDDEN");
}

#[tokio::test]
async fn empty_password_pkcs12_remains_importable_after_portable_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let (pkcs12, _, _) = client_pkcs12_with_password("");
    let path = directory.path().join("empty-password.p12");
    std::fs::write(&path, pkcs12).unwrap();
    let adapter = ManagedListenerCertificateAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::from([path])))),
    );
    let imported = adapter
        .import_upstream_client_identity("空密码身份".into(), String::new())
        .await
        .unwrap()
        .unwrap()
        .reference;

    let portable = adapter.export_portable(imported).await.unwrap();
    assert_eq!(portable.password.as_deref(), Some(""));

    let restored = adapter.restore_portable(portable).await.unwrap();
    assert!(adapter.inspect(restored).await.is_ok());
}

#[tokio::test]
async fn pem_client_identity_imports_without_password_and_survives_portable_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let (pem, private_key, _) = client_identity_pem();
    let path = directory.path().join("client.pem");
    std::fs::write(&path, &pem).unwrap();
    let adapter = ManagedListenerCertificateAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::from([path])))),
    );

    let imported = adapter
        .import_upstream_client_identity("PEM 客户端身份".into(), "ignored".into())
        .await
        .unwrap()
        .unwrap()
        .reference;
    let resolved = adapter.resolve_identity(&imported).unwrap().unwrap();
    assert_eq!(resolved.private_key_pkcs8_der.as_slice(), private_key);

    let portable = adapter.export_portable(imported).await.unwrap();
    assert_eq!(portable.password, None);
    assert_eq!(STANDARD.decode(&portable.material_base64).unwrap(), pem);

    let restored = adapter.restore_portable(portable).await.unwrap();
    assert!(adapter.inspect(restored).await.is_ok());
}

fn assert_imported_certificate_subjects(
    identity: &ListenerCertificateImportViewModel,
    downstream_identity: &ListenerCertificateImportViewModel,
    trust: &ListenerCertificateImportViewModel,
) {
    assert_eq!(
        identity.detail.certificate.as_ref().unwrap().subject,
        "CN=Listener Client"
    );
    assert_eq!(
        downstream_identity
            .detail
            .certificate
            .as_ref()
            .unwrap()
            .subject,
        "CN=listener.test"
    );
    assert_eq!(
        trust.detail.certificate.as_ref().unwrap().subject,
        "CN=Listener Client Root"
    );
}

fn assert_references_are_protected(
    store: &SqliteStore,
    references: &[&CertificateReference],
    imported_pkcs12: &[u8],
) {
    for reference in references {
        let key = managed_key(&reference.reference).unwrap().unwrap();
        let record = store.load_protected_secret(PROVIDER, key).unwrap().unwrap();
        assert!(
            !record
                .protected_blob
                .windows(imported_pkcs12.len())
                .any(|window| window == imported_pkcs12)
        );
        assert!(
            !record
                .protected_blob
                .windows(8)
                .any(|window| window == b"password")
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn assert_resolved_material(
    adapter: &ManagedListenerCertificateAdapter,
    identity_a: &CertificateReference,
    trust_b: &CertificateReference,
    downstream_identity: &CertificateReference,
    downstream_trust: &CertificateReference,
    private_key: &[u8],
    ca_der: &[u8],
    server_private_key: &[u8],
    downstream_ca_der: &[u8],
) {
    let resolved = adapter.resolve_identity(identity_a).unwrap().unwrap();
    assert_eq!(resolved.private_key_pkcs8_der.as_slice(), private_key);
    assert_eq!(
        adapter.resolve_trust(trust_b).unwrap().unwrap(),
        vec![ca_der.to_vec()]
    );
    let resolved = adapter
        .resolve_identity(downstream_identity)
        .unwrap()
        .unwrap();
    assert_eq!(
        resolved.private_key_pkcs8_der.as_slice(),
        server_private_key
    );
    assert_eq!(
        adapter.resolve_trust(downstream_trust).unwrap().unwrap(),
        vec![downstream_ca_der.to_vec()]
    );
}

async fn assert_inspected_details(
    adapter: &ManagedListenerCertificateAdapter,
    identity: CertificateReference,
    trust: CertificateReference,
) {
    let identity_detail = adapter.inspect(identity).await.unwrap();
    assert_eq!(
        identity_detail.usage,
        "代理向上游服务器出示的 mTLS 客户端身份"
    );
    assert!(!identity_detail.sha256_fingerprint.is_empty());
    let trust_detail = adapter.inspect(trust).await.unwrap();
    assert_eq!(trust_detail.usage, "验证上游服务器证书的 CA");
    assert_eq!(trust_detail.status_text, "有效");
}

#[tokio::test]
async fn inspects_file_references_without_returning_file_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = CertificateService
        .generate_root_ca("Downstream Client Root")
        .unwrap();
    let path = directory.path().join("client-root.crt");
    std::fs::write(&path, &root.certificate_der).unwrap();
    let adapter = ManagedListenerCertificateAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::new()))),
    );
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "终端客户端 CA".into(),
        kind: CertificateReferenceKind::DownstreamClientTrust,
        reference: format!("file:{}", path.display()),
    };

    let detail = adapter.inspect(reference).await.unwrap();

    assert_eq!(detail.subject, "CN=Downstream Client Root");
    assert_eq!(detail.usage, "验证客户端证书的 CA");
    assert!(!format!("{detail:?}").contains(path.to_str().unwrap()));
}

#[tokio::test]
async fn managed_reference_cannot_relabel_material_as_another_certificate_role() {
    let directory = tempfile::tempdir().unwrap();
    let (_, _, ca_der) = client_pkcs12();
    let trust_path = directory.path().join("server-ca.crt");
    std::fs::write(&trust_path, ca_der).unwrap();
    let adapter = ManagedListenerCertificateAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::from([trust_path])))),
    );
    let imported = adapter
        .import_upstream_server_trust("上游 Server CA".into())
        .await
        .unwrap()
        .unwrap();
    let mut forged = imported.reference;
    forged.kind = CertificateReferenceKind::DownstreamClientTrust;

    let error = adapter.inspect(forged).await.unwrap_err();

    assert_eq!(error.view_model.code, "CERTIFICATE_NOT_READY");
    assert!(error.view_model.message.contains("材料类型不匹配"));
}

#[tokio::test]
async fn cancelling_native_dialog_returns_none_without_persisting_a_reference() {
    let adapter = ManagedListenerCertificateAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::new()))),
    );

    assert!(
        adapter
            .import_downstream_server_identity("server".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        adapter
            .import_downstream_client_trust("client ca".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        adapter
            .import_upstream_client_identity("identity".into(), "password".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        adapter
            .import_upstream_server_trust("trust".into())
            .await
            .unwrap()
            .is_none()
    );
}
