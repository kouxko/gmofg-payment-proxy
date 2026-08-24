use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use p12_keystore::{Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain};
use parking_lot::Mutex as ParkingMutex;
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
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

    fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
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

    fn choose_save_file(&self, purpose: &str, _: &str) -> AppResult<Option<FileSelection>> {
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
    let client_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("client identity key");
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

#[test]
fn installation_identity_preserves_keychain_unprotect_error_code() {
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
        .load_installation_server_identity()
        .expect_err("identity load must fail");

    assert_eq!(error.view_model.code, "KEYCHAIN_UNPROTECT_FAILED");
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
