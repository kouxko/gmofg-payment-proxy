use super::*;

#[test]
fn cache_hit_observes_deletion_performed_by_another_repository_adapter() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let cached = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    let deleting = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    cached.install_zip(&package_zip(MANIFEST, SCRIPT)).unwrap();
    let package = package("1.0.0");
    assert!(cached.compiled(&package).is_ok());

    deleting.delete(&package).unwrap();

    let error = cached.compiled(&package).unwrap_err();
    assert_eq!(error.code(), ProtocolPackageStorageErrorCode::NotFound);
    assert_eq!(error.detail_code(), Some("PROTOCOL_PACKAGE_NOT_FOUND"));
}

#[test]
fn cache_generation_changes_when_another_adapter_deletes_and_reinstalls_same_identity() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let cached = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    let replacing = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    cached.install_zip(&package_zip(MANIFEST, SCRIPT)).unwrap();
    let package = package("1.0.0");
    let old_compiled = cached.compiled(&package).unwrap();

    replacing.delete(&package).unwrap();
    let replacement_script = SCRIPT.replace("{ origin }", "{ blob() }");
    assert!(matches!(
        replacing
            .install_zip(&package_zip(MANIFEST, &replacement_script))
            .unwrap(),
        ProtocolPackageInstallOutcome::Installed(_)
    ));

    let new_compiled = cached.compiled(&package).unwrap();
    assert!(
        !Arc::ptr_eq(&old_compiled, &new_compiled),
        "a new persisted generation must never reuse the deleted AST"
    );
    assert_eq!(new_compiled.package(), &package);
}
