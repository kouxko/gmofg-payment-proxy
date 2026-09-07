use base64::engine::general_purpose::STANDARD;

use super::*;

#[tokio::test]
async fn upstream_ca_bundle_persists_and_resolves_every_member_after_adapter_restart() {
    let directory = tempfile::tempdir().unwrap();
    let first = CertificateService
        .generate_root_ca("Persisted Bundle First")
        .unwrap();
    let second = CertificateService
        .generate_root_ca("Persisted Bundle Second")
        .unwrap();
    let bundle_path = directory.path().join("upstream-bundle.pem");
    std::fs::write(
        &bundle_path,
        pem_certificate_bundle(&[&first.certificate_der, &second.certificate_der]),
    )
    .unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let adapter = ManagedListenerCertificateAdapter::new(
        store.clone(),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::from([bundle_path])))),
    );
    let reference = adapter
        .import_upstream_server_trust("Two-member trust".into())
        .await
        .unwrap()
        .unwrap()
        .reference;

    let restarted = ManagedListenerCertificateAdapter::new(
        store,
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::new()))),
    );
    let resolved = restarted.resolve_trust(&reference).await.unwrap().unwrap();

    assert_eq!(
        resolved,
        vec![
            first.certificate_der.clone(),
            second.certificate_der.clone()
        ]
    );
}

fn pem_certificate_bundle(certificates: &[&[u8]]) -> Vec<u8> {
    let mut pem = String::new();
    for certificate in certificates {
        pem.push_str("-----BEGIN CERTIFICATE-----\n");
        pem.push_str(&STANDARD.encode(certificate));
        pem.push_str("\n-----END CERTIFICATE-----\n");
    }
    pem.into_bytes()
}
