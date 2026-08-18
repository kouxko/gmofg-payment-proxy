#[tokio::test]
async fn downstream_server_pkcs12_is_normalized_without_persisting_container_or_password() {
    let directory = tempfile::tempdir().unwrap();
    let (pkcs12, private_key, _) = server_pkcs12_with_password("server-secret");
    let path = directory.path().join("server.pfx");
    std::fs::write(&path, &pkcs12).unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let adapter = ManagedListenerCertificateAdapter::new(
        store.clone(),
        Arc::new(XorProtector),
        Arc::new(QueueDialog(Mutex::new(VecDeque::from([path])))),
    );

    let imported = adapter
        .import_downstream_server_identity("服务端身份".into(), "server-secret".into())
        .await
        .unwrap()
        .unwrap()
        .reference;
    let key = managed_key(&imported.reference).unwrap().unwrap();
    let record = store.load_protected_secret(PROVIDER, key).unwrap().unwrap();
    let plaintext = Zeroizing::new(XorProtector.unprotect(&record.protected_blob).unwrap());
    let material = decode_material(plaintext).unwrap();

    assert_eq!(material.kind, KIND_DOWNSTREAM_SERVER_IDENTITY);
    assert!(material.password.is_empty());
    assert!(material.bytes.starts_with(b"-----BEGIN CERTIFICATE-----"));
    assert!(
        !material
            .bytes
            .windows(pkcs12.len())
            .any(|window| window == pkcs12)
    );
    assert!(
        !material
            .bytes
            .windows(13)
            .any(|window| window == b"server-secret")
    );
    let resolved = adapter.resolve_identity(&imported).unwrap().unwrap();
    assert_eq!(resolved.private_key_pkcs8_der.as_slice(), private_key);
}

#[tokio::test]
async fn downstream_server_pkcs12_rejects_wrong_and_accepts_empty_password() {
    let directory = tempfile::tempdir().unwrap();
    let (protected, _, _) = server_pkcs12_with_password("correct");
    let (empty, _, _) = server_pkcs12_with_password("");
    let protected_path = directory.path().join("protected.p12");
    let empty_path = directory.path().join("empty.p12");
    std::fs::write(&protected_path, protected).unwrap();
    std::fs::write(&empty_path, empty).unwrap();
    let protector = Arc::new(CountingProtector::default());
    let adapter = ManagedListenerCertificateAdapter::new(
        Arc::new(SqliteStore::in_memory().unwrap()),
        protector.clone(),
        Arc::new(QueueDialog(Mutex::new(VecDeque::from([
            protected_path,
            empty_path,
        ])))),
    );

    let error = adapter
        .import_downstream_server_identity("错误密码".into(), "wrong".into())
        .await
        .unwrap_err();
    assert_eq!(error.view_model.code, "PKCS12_PASSWORD_INVALID");
    assert_eq!(
        protector.0.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "failed import must not persist protected material"
    );
    assert!(
        adapter
            .import_downstream_server_identity("空密码".into(), String::new())
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(protector.0.load(std::sync::atomic::Ordering::SeqCst), 1);
}
#[derive(Debug, Default)]
struct CountingProtector(std::sync::atomic::AtomicUsize);

impl SecretProtector for CountingProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        XorProtector.protect(plaintext)
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        XorProtector.unprotect(ciphertext)
    }
}
