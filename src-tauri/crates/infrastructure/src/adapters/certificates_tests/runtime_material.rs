#[tokio::test]
async fn generic_profile_exports_the_fixed_test_root_without_private_material() {
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
        .expect("fixed test signing material is initialized");
    adapter.export_ca().await.expect("public-only export");
    let exported = std::fs::read(export_path).expect("exported public certificate");
    assert!(exported.starts_with(b"-----BEGIN CERTIFICATE-----"));
    assert!(!exported.windows(11).any(|window| window == b"PRIVATE KEY"));
}

// CERT-005~017, SECURITY-006~009, TEST-TLS
#[tokio::test]
async fn protected_material_supports_listener_tls_configuration() {
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
    assert!(duplicate.view_model.message.contains("Root CA 已经初始化"));
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
async fn separate_proxy_installations_share_the_fixed_root_but_keep_distinct_leaves() {
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

    assert_eq!(first_root.certificate_der, second_root.certificate_der);
    assert_eq!(first_root.private_key_der, second_root.private_key_der);
    assert_eq!(first_root.fingerprint, second_root.fingerprint);
    assert_eq!(
        first_root.fingerprint,
        "B4:72:77:A5:8D:81:AD:EB:3C:CE:59:7A:15:58:85:4D:AB:3D:0B:30:AB:CE:15:06:5A:FB:73:33:9B:CB:D7:4C"
    );
    assert_ne!(first_leaf.certificate_der, second_leaf.certificate_der);
    assert_ne!(first_leaf.private_key_der, second_leaf.private_key_der);
    assert_eq!(first_leaf.sans, vec!["IP:10.0.34.50"]);
    assert_eq!(second_leaf.sans, vec!["IP:10.0.28.99"]);
}

#[tokio::test]
async fn startup_synchronization_is_idempotent_for_the_fixed_test_root() {
    let adapter = CertificateServiceAdapter::new(
        Arc::new(SqliteStore::in_memory().expect("store")),
        Arc::new(XorProtector),
        Arc::new(QueueDialog {
            open: ParkingMutex::new(VecDeque::new()),
        }),
        test_profile(),
    );

    let first = adapter
        .synchronize_installation_ca(vec!["10.0.34.50".into()])
        .await
        .expect("first synchronization");
    let second = adapter
        .synchronize_installation_ca(vec!["127.0.0.1".into()])
        .await
        .expect("second synchronization");

    assert_eq!(first.revision, second.revision);
    let root = adapter
        .load_snapshot(&[ROOT])
        .expect("fixed root snapshot")
        .materials
        .remove(ROOT)
        .expect("fixed root");
    assert_eq!(
        root.fingerprint,
        "B4:72:77:A5:8D:81:AD:EB:3C:CE:59:7A:15:58:85:4D:AB:3D:0B:30:AB:CE:15:06:5A:FB:73:33:9B:CB:D7:4C"
    );
}

#[tokio::test]
async fn startup_synchronization_rejects_non_current_root_material() {
    let adapter = CertificateServiceAdapter::new(
        Arc::new(SqliteStore::in_memory().expect("store")),
        Arc::new(XorProtector),
        Arc::new(QueueDialog {
            open: ParkingMutex::new(VecDeque::new()),
        }),
        test_profile(),
    );
    let legacy_root = adapter
        .certificates
        .generate_root_ca("Legacy Per-install Root CA")
        .expect("legacy root");
    let legacy_leaf = adapter
        .certificates
        .generate_leaf(
            &legacy_root.certificate_der,
            &legacy_root.private_key_pkcs8_der,
            &leaf_request(&["10.0.34.50".into(), "proxy.test".into()]).expect("leaf request"),
        )
        .expect("legacy leaf");
    let mut legacy_snapshot = adapter.load_snapshot(&MATERIAL_KINDS).expect("snapshot");
    legacy_snapshot
        .materials
        .insert(ROOT.into(), from_bundle(1, &legacy_root));
    legacy_snapshot
        .materials
        .insert(LEAF.into(), from_bundle(1, &legacy_leaf));
    adapter
        .commit_snapshot(legacy_snapshot)
        .expect("store legacy installation");

    let error = adapter
        .synchronize_installation_ca(vec!["127.0.0.1".into()])
        .await
        .expect_err("non-current root must be rejected");
    assert_eq!(
        error.view_model.code,
        "CERTIFICATE_INSTALLATION_STATE_INVALID"
    );

    let unchanged = adapter
        .load_snapshot(&[ROOT, LEAF])
        .expect("unchanged snapshot");
    assert_eq!(
        unchanged
            .materials
            .get(ROOT)
            .expect("stored root")
            .fingerprint,
        legacy_root.metadata.fingerprint_sha256
    );
    assert_eq!(
        unchanged
            .materials
            .get(LEAF)
            .expect("stored leaf")
            .fingerprint,
        legacy_leaf.metadata.fingerprint_sha256
    );
}
