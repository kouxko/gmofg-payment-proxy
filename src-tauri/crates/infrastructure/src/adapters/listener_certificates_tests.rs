use std::{collections::VecDeque, path::PathBuf};

use p12_keystore::{Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain};
use parking_lot::Mutex;
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
};

use super::*;
use crate::{InfrastructureError, adapters::FileSelection};

#[derive(Debug)]
struct QueueDialog(Mutex<VecDeque<PathBuf>>);

impl NativeFileDialog for QueueDialog {
    fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
        Ok(self.0.lock().pop_front())
    }

    fn choose_save_file(&self, _: &str) -> AppResult<Option<FileSelection>> {
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
    let first_identity = directory.path().join("first.p12");
    let second_identity = directory.path().join("second.p12");
    let first_trust = directory.path().join("first.crt");
    let second_trust = directory.path().join("second.crt");
    std::fs::write(&first_identity, &pkcs12).unwrap();
    std::fs::write(&second_identity, &pkcs12).unwrap();
    std::fs::write(&first_trust, &ca_der).unwrap();
    std::fs::write(&second_trust, &ca_der).unwrap();

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let adapter = ManagedListenerCertificateAdapter::new(
        store.clone(),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::from([
            first_identity,
            second_identity,
            first_trust,
            second_trust,
        ])))),
    );

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

    assert_eq!(
        identity_a.detail.certificate.as_ref().unwrap().subject,
        "CN=Listener Client"
    );
    assert_eq!(
        trust_a.detail.certificate.as_ref().unwrap().subject,
        "CN=Listener Client Root"
    );
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

    for reference in [&identity_a, &identity_b, &trust_a, &trust_b] {
        let key = managed_key(&reference.reference).unwrap().unwrap();
        let record = store.load_protected_secret(PROVIDER, key).unwrap().unwrap();
        assert!(
            !record
                .protected_blob
                .windows(pkcs12.len())
                .any(|window| window == pkcs12)
        );
        assert!(
            !record
                .protected_blob
                .windows(8)
                .any(|window| window == b"password")
        );
    }

    let resolved = adapter.resolve_identity(&identity_a).unwrap().unwrap();
    assert_eq!(resolved.private_key_pkcs8_der.as_slice(), private_key);
    assert_eq!(
        adapter.resolve_trust(&trust_b).unwrap().unwrap(),
        vec![ca_der]
    );
    let identity_detail = adapter.inspect(identity_a).await.unwrap();
    assert_eq!(
        identity_detail.usage,
        "代理向上游服务器出示的 mTLS 客户端身份"
    );
    assert!(!identity_detail.sha256_fingerprint.is_empty());
    let trust_detail = adapter.inspect(trust_b).await.unwrap();
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
async fn cancelling_native_dialog_returns_none_without_persisting_a_reference() {
    let adapter = ManagedListenerCertificateAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::new()))),
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

fn client_pkcs12() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let root = CertificateService
        .generate_root_ca("Listener Client Root")
        .unwrap();
    let root_key = KeyPair::from_pkcs8_der_and_sign_algo(
        &root.private_key_pkcs8_der.as_slice().into(),
        &PKCS_ECDSA_P256_SHA256,
    )
    .unwrap();
    let issuer =
        Issuer::from_ca_cert_der(&root.certificate_der.as_slice().into(), root_key).unwrap();
    let client_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Listener Client");
    params.distinguished_name = name;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let certificate = params.signed_by(&client_key, &issuer).unwrap();
    let private_key = client_key.serialize_der();
    let mut keystore = KeyStore::new();
    keystore.add_entry(
        "listener",
        KeyStoreEntry::PrivateKeyChain(PrivateKeyChain::new(
            "listener-key",
            PrivateKey::from_der(&private_key).unwrap(),
            [
                Certificate::from_der(certificate.der()).unwrap(),
                Certificate::from_der(&root.certificate_der).unwrap(),
            ],
        )),
    );
    (
        keystore.writer("password").write().unwrap(),
        private_key,
        root.certificate_der.clone(),
    )
}
