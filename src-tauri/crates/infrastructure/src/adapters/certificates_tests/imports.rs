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
