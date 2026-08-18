use super::*;

/// 完整配置提交也是公开端口，必须独立执行同一套 fresh binding 门禁。
#[tokio::test]
async fn direct_application_bundle_cannot_bypass_fresh_binding_validation() {
    let source = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::in_memory().unwrap(),
    ));
    source.install_zip(&package_zip(SCRIPT)).unwrap();
    let packages = source.export_application_packages().await.unwrap();
    let document = application_document(workspace_requiring_upstream_encode(), packages.clone());
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));

    let error = repository
        .replace_application_bundle(packages, document)
        .await
        .unwrap_err();

    assert_eq!(
        error.view_model.code,
        "PROTOCOL_PACKAGE_CAPABILITY_MISMATCH"
    );
    assert!(repository.list().unwrap().is_empty());
    assert!(store.load_workspaces().unwrap().records.is_empty());
}
