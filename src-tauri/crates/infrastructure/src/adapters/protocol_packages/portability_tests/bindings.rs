use super::*;

/// 完整配置提交也是公开端口，必须独立执行同一套 fresh binding 门禁。
#[tokio::test]
async fn direct_application_bundle_cannot_bypass_fresh_binding_validation() {
    let source = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::in_memory().unwrap(),
    ));
    let http_manifest = MANIFEST.replace("frame = \"frame\"\n", "");
    source
        .install_zip(&package_zip_with_manifest(&http_manifest, SCRIPT))
        .unwrap();
    let packages = source.export_application_packages().await.unwrap();
    let document = application_document(socket_workspace_with_package(), packages.clone());
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));

    let error = repository
        .replace_application_bundle(packages, document)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "PORTABLE_PROTOCOL_PACKAGE_INVALID");
    assert!(repository.list().unwrap().is_empty());
    assert!(store.load_workspaces().unwrap().records.is_empty());
}
