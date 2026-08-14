use super::*;

#[tokio::test]
async fn legacy_workspace_commit_preserves_registry_identity_and_enabled_state() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository.install_zip(&package_zip(SCRIPT)).unwrap();
    repository.set_enabled(&package(), true).unwrap();
    let before = store
        .load_protocol_package_header(&package())
        .unwrap()
        .unwrap();
    let workspace = workspace_with_package();

    repository
        .commit_legacy_workspace(workspace.clone())
        .await
        .unwrap();

    let after = store
        .load_protocol_package_header(&package())
        .unwrap()
        .unwrap();
    assert_eq!(after, before);
    assert_eq!(
        store.load_workspaces().unwrap().records[0].id,
        workspace.id.as_uuid()
    );
}

#[tokio::test]
async fn legacy_workspace_missing_reference_is_zero_write() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));

    let error = repository
        .commit_legacy_workspace(workspace_with_package())
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_NOT_FOUND");
    assert!(store.load_workspaces().unwrap().records.is_empty());
    assert!(repository.list().unwrap().is_empty());
}

#[tokio::test]
async fn legacy_application_ignores_unreferenced_bad_package_and_preserves_registry() {
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
    store.replace_protocol_package_file_for_test(&extra_ref, "protocol.rhai", b"fn frame( {");
    store
        .set_protocol_package_validation(&extra_ref, Some("SCRIPT_SYNTAX_INVALID"))
        .unwrap();
    let before_headers = store.list_protocol_package_headers().unwrap();
    let before_cache = cache_snapshot(&repository);
    let workspace = workspace_with_package();
    let workspace_id = workspace.id;
    let mut document = application_document(workspace, Vec::new());
    document.settings = PortableSettings::from(&SettingsDraft {
        max_sessions: 999,
        ..SettingsDraft::default()
    });

    repository
        .replace_legacy_application_configuration(document)
        .await
        .unwrap();

    assert_eq!(
        store.list_protocol_package_headers().unwrap(),
        before_headers
    );
    assert_eq!(
        store.load_workspaces().unwrap().selected_id,
        Some(workspace_id.as_uuid())
    );
    assert_eq!(
        store.load_settings().unwrap().unwrap().value["max_sessions"],
        999
    );
    assert_cache_unchanged(&repository, &before_cache);
}

#[tokio::test]
async fn legacy_sqlite_compare_detects_content_change_before_workspace_insert() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository.install_zip(&package_zip(SCRIPT)).unwrap();
    let portable = repository
        .export_workspace_packages(&[package()])
        .await
        .unwrap();
    let prepared = repository.prepare_workspace_packages(&portable).unwrap();
    let writes = prepared_into_writes(prepared, false);
    store.replace_protocol_package_file_for_test(
        &package(),
        "protocol.rhai",
        SCRIPT.replace("create()", "create()\n").as_bytes(),
    );
    let record = WorkspaceRepositoryAdapter::record(&workspace_with_package()).unwrap();

    let error = store.insert_legacy_workspace(&record, &writes).unwrap_err();

    assert!(matches!(
        error,
        StoredProtocolPackageBundleError::IdentityConflict(_)
    ));
    assert!(store.load_workspaces().unwrap().records.is_empty());
}

/// 历史文档的首次 preflight 不能作为提交凭证。另一进程即使把同 identity 替换成
/// 仍可编译但缺少所需入口的内容，Workspace 与完整配置提交也必须使用 fresh 描述拒绝。
#[tokio::test]
async fn legacy_commits_recheck_bindings_after_valid_same_identity_replacement() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository
        .install_zip(&package_zip_with_manifest(
            MANIFEST_WITH_UPSTREAM_ENCODE,
            SCRIPT_WITH_ENCODE,
        ))
        .unwrap();
    let old_descriptions = repository
        .preflight_installed_packages(&[package()])
        .await
        .unwrap();
    assert!(old_descriptions[0].capabilities.upstream.encode);

    // 模拟另一个进程在 preflight 后替换持久化文件。新内容身份相同且仍能编译，但不再
    // 提供 Listener 已启用的 upstream Encode；事务内 exact compare 本身无法识别此错配。
    store.replace_protocol_package_file_for_test(&package(), "manifest.toml", MANIFEST.as_bytes());
    let before_workspaces = store.load_workspaces().unwrap();
    let before_settings = store.load_settings().unwrap();

    let workspace_error = repository
        .commit_legacy_workspace(workspace_requiring_upstream_encode())
        .await
        .unwrap_err();
    assert_eq!(
        workspace_error.view_model.code,
        "PROTOCOL_PACKAGE_CAPABILITY_MISMATCH"
    );
    assert_eq!(store.load_workspaces().unwrap(), before_workspaces);

    let document = application_document(workspace_requiring_upstream_encode(), Vec::new());
    let application_error = repository
        .replace_legacy_application_configuration(document)
        .await
        .unwrap_err();
    assert_eq!(
        application_error.view_model.code,
        "PROTOCOL_PACKAGE_CAPABILITY_MISMATCH"
    );
    assert_eq!(store.load_workspaces().unwrap(), before_workspaces);
    assert_eq!(store.load_settings().unwrap(), before_settings);
}

#[tokio::test]
async fn installed_preflight_never_mutates_header_or_warm_cache() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository.install_zip(&package_zip(SCRIPT)).unwrap();
    repository.compiled(&package()).unwrap();
    store
        .set_protocol_package_validation(&package(), Some("STALE_INVALID"))
        .unwrap();
    let success_header = store
        .load_protocol_package_header(&package())
        .unwrap()
        .unwrap();
    let success_cache = cache_snapshot(&repository);

    let descriptions = repository
        .preflight_installed_packages(&[package()])
        .await
        .unwrap();

    assert_eq!(descriptions[0].package, package());
    assert_eq!(
        store
            .load_protocol_package_header(&package())
            .unwrap()
            .unwrap(),
        success_header
    );
    assert_cache_unchanged(&repository, &success_cache);

    store.replace_protocol_package_file_for_test(&package(), "protocol.rhai", b"fn frame( {");
    let failure_header = store
        .load_protocol_package_header(&package())
        .unwrap()
        .unwrap();
    let failure_cache = cache_snapshot(&repository);
    assert!(
        repository
            .preflight_installed_packages(&[package()])
            .await
            .is_err()
    );
    assert_eq!(
        store
            .load_protocol_package_header(&package())
            .unwrap()
            .unwrap(),
        failure_header
    );
    assert_cache_unchanged(&repository, &failure_cache);
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
