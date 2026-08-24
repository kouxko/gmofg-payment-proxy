use super::*;

#[tokio::test]
async fn full_replace_late_conflict_restores_every_sqlite_and_cache_snapshot() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository.install_zip(&package_zip(SCRIPT)).unwrap();
    repository.set_enabled(&package(), true).unwrap();
    let extra_ref = ProtocolPackageRef {
        id: package().id,
        version: ProtocolPackageVersion::new("2.0.0").unwrap(),
    };
    let extra_manifest = MANIFEST.replace("1.0.0", "2.0.0");
    repository
        .install_zip(&package_zip_with_manifest(&extra_manifest, SCRIPT))
        .unwrap();
    repository.compiled(&package()).unwrap();
    repository.compiled(&extra_ref).unwrap();
    store
        .execute_test_batch(
            "INSERT INTO external_protocol_packages(
                package_id, version, registration_json, registration_fingerprint,
                enabled, first_connected_at, last_connected_at
             ) VALUES (
                'portable-test', '1.0.0', '{}', zeroblob(32), 0,
                '2026-08-22T00:00:00Z', '2026-08-22T00:00:00Z'
             );",
        )
        .unwrap();

    let baseline_document = application_document(ProxyWorkspace::default(), Vec::new());
    let (records, settings) = application_records(&baseline_document).unwrap();
    store
        .replace_application_configuration(
            baseline_document.selected_workspace_id.as_uuid(),
            &records,
            &settings,
        )
        .unwrap();

    let original = repository
        .export_application_packages()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.package == package())
        .unwrap();
    let mut missing = original.clone();
    missing.package.id = ProtocolPackageId::new("aaa-new-package").unwrap();
    missing
        .files
        .iter_mut()
        .find(|file| file.path == "manifest.toml")
        .unwrap()
        .contents_base64 = STANDARD.encode(MANIFEST.replace("portable-test", "aaa-new-package"));
    let mut conflicting = original;
    conflicting
        .files
        .iter_mut()
        .find(|file| file.path == "protocol.rhai")
        .unwrap()
        .contents_base64 = STANDARD.encode(SCRIPT.replace("create()", "create()\n"));
    let packages = vec![missing.clone(), conflicting];
    let replacement = application_document(
        workspace_with_packages(&[missing.package.clone(), package()]),
        packages.clone(),
    );
    let before_registry = repository.list().unwrap();
    let before_files = repository.export_application_packages().await.unwrap();
    let before_workspaces = store.load_workspaces().unwrap();
    let before_settings = store.load_settings().unwrap();
    let before_cache = cache_snapshot(&repository);

    let error = repository
        .replace_application_bundle(packages, replacement)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_IDENTITY_CONFLICT");
    assert_eq!(repository.list().unwrap(), before_registry);
    assert_eq!(
        repository.export_application_packages().await.unwrap(),
        before_files
    );
    assert_eq!(store.load_workspaces().unwrap(), before_workspaces);
    assert_eq!(store.load_settings().unwrap(), before_settings);
    assert_cache_unchanged(&repository, &before_cache);
}

type CacheSnapshot = Vec<(
    ProtocolPackageRef,
    uuid::Uuid,
    Arc<intercept_proxy_protocol_scripting::CompiledProtocolPackage>,
)>;

fn cache_snapshot(repository: &ProtocolPackageRepositoryAdapter) -> CacheSnapshot {
    repository
        .cache
        .lock()
        .iter()
        .map(|(package, cached)| {
            (
                package.clone(),
                cached.generation,
                Arc::clone(&cached.compiled),
            )
        })
        .collect()
}

fn assert_cache_unchanged(repository: &ProtocolPackageRepositoryAdapter, before: &CacheSnapshot) {
    let cache = repository.cache.lock();
    assert_eq!(cache.len(), before.len());
    for (package, generation, compiled) in before {
        let after = cache.get(package).expect("cached identity must remain");
        assert_eq!(&after.generation, generation);
        assert!(Arc::ptr_eq(&after.compiled, compiled));
    }
}
