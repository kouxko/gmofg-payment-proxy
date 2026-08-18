use intercept_proxy_application::{
    ProtocolPackageCompilerPort, ProtocolPackageStorePort, ProtocolPackageValidationViewModel,
};

use super::*;

#[tokio::test]
async fn application_store_and_compiler_ports_preserve_exact_versions_and_safe_models() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(store);
    let ProtocolPackageInstallOutcome::Installed(first) = repository
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap()
    else {
        panic!("first import must install");
    };
    let manifest_v2 = MANIFEST.replace("version = \"1.0.0\"", "version = \"2.0.0\"");
    repository
        .install_zip(&package_zip(&manifest_v2, SCRIPT))
        .unwrap();

    ProtocolPackageStorePort::set_enabled(&repository, &first.package, true)
        .await
        .unwrap();
    let versions = ProtocolPackageStorePort::list(&repository).await.unwrap();

    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].package, package("1.0.0"));
    assert!(versions[0].enabled);
    assert_eq!(versions[0].name, "Example Protocol");
    assert_eq!(versions[0].host_api, 1);
    assert_eq!(
        versions[0].validation,
        ProtocolPackageValidationViewModel::Valid
    );
    assert_eq!(versions[1].package, package("2.0.0"));
    assert!(!versions[1].enabled);

    let receipt = ProtocolPackageCompilerPort::compile_fresh(&repository, &first.package)
        .await
        .unwrap();
    assert_eq!(receipt.package, first.package);
    assert_eq!(receipt.host_api, 1);
    assert!(receipt.compatible);
}

#[tokio::test]
async fn application_ports_map_missing_and_invalid_storage_to_stable_errors() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let installer = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    installer
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    store.replace_protocol_package_file_for_test(
        &package("1.0.0"),
        "protocol.rhai",
        b"fn frame( {",
    );
    let restarted = ProtocolPackageRepositoryAdapter::with_default_limits(store);

    let invalid = ProtocolPackageCompilerPort::compile_fresh(&restarted, &package("1.0.0"))
        .await
        .unwrap_err();
    assert_eq!(invalid.view_model.code, "SCRIPT_SYNTAX_INVALID");
    assert_eq!(
        invalid.view_model.entity_id.as_deref(),
        Some("example-protocol@1.0.0")
    );
    assert!(!invalid.view_model.retryable);

    let missing = ProtocolPackageStorePort::delete(&restarted, &package("9.0.0"))
        .await
        .unwrap_err();
    assert_eq!(missing.view_model.code, "PROTOCOL_PACKAGE_NOT_FOUND");
    assert_eq!(
        missing.view_model.entity_id.as_deref(),
        Some("example-protocol@9.0.0")
    );
    assert!(!missing.view_model.retryable);

    let stored = ProtocolPackageStorePort::get(&restarted, &package("1.0.0"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.validation,
        ProtocolPackageValidationViewModel::Invalid {
            code: "SCRIPT_SYNTAX_INVALID".into(),
        }
    );
}

#[tokio::test]
async fn enable_validation_bypasses_a_warm_ast_cache_and_keeps_enabled_false_on_damage() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    repository
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let target = package("1.0.0");
    // 先明确暖缓存；随后直接模拟同一 generation 的持久化文件损坏。
    assert!(repository.compiled(&target).is_ok());
    store.replace_protocol_package_file_for_test(&target, "protocol.rhai", b"fn frame( {");

    let error = ProtocolPackageCompilerPort::compile_fresh(&repository, &target)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "SCRIPT_SYNTAX_INVALID");
    let stored = ProtocolPackageStorePort::get(&repository, &target)
        .await
        .unwrap()
        .unwrap();
    assert!(!stored.enabled);
    assert_eq!(
        stored.validation,
        ProtocolPackageValidationViewModel::Invalid {
            code: "SCRIPT_SYNTAX_INVALID".into(),
        }
    );
}

#[test]
fn every_storage_error_family_maps_to_a_stable_source_free_application_error() {
    let archive = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::in_memory().unwrap(),
    ))
    .install_zip(b"not a zip")
    .unwrap_err();
    assert_mapped_error(&archive, archive.detail_code().unwrap(), false);

    let compilation = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::in_memory().unwrap(),
    ))
    .install_zip(&package_zip(MANIFEST, "fn frame( {"))
    .unwrap_err();
    assert_mapped_error(&compilation, "SCRIPT_SYNTAX_INVALID", false);

    let conflict_repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::in_memory().unwrap(),
    ));
    conflict_repository
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap();
    let replacement = SCRIPT.replace("{ origin }", "{ blob() }");
    let conflict = conflict_repository
        .install_zip(&package_zip(MANIFEST, &replacement))
        .unwrap_err();
    assert_mapped_error(&conflict, "PROTOCOL_PACKAGE_IDENTITY_CONFLICT", false);

    let failing_store = Arc::new(SqliteStore::in_memory().unwrap());
    failing_store.reject_protocol_package_file_for_test("protocol.rhai");
    let persistence = ProtocolPackageRepositoryAdapter::with_default_limits(failing_store)
        .install_zip(&package_zip(MANIFEST, SCRIPT))
        .unwrap_err();
    assert_mapped_error(&persistence, "PROTOCOL_PACKAGE_PERSISTENCE_FAILED", true);
}

fn assert_mapped_error(
    storage: &ProtocolPackageStorageError,
    expected_code: &str,
    retryable: bool,
) {
    let mapped = super::super::application_port::protocol_package_app_error(storage);
    assert_eq!(mapped.view_model.code, expected_code);
    assert_eq!(mapped.view_model.retryable, retryable);
    assert!(
        !mapped.view_model.message.contains("fn frame"),
        "Rhai source must never cross the Application error boundary"
    );
}
