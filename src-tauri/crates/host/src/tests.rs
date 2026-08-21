use std::path::PathBuf;

use intercept_proxy_application::{
    AppResult, ExternalPackageServiceStateViewModel, SettingsRepositoryPort,
};
use intercept_proxy_infrastructure::{
    CURRENT_APPLICATION_SCHEMA_VERSION, InfrastructureError, NativeFileDialog, SecretProtector,
    SettingsRepositoryAdapter, SqliteStore, adapters::FileSelection,
};
use intercept_proxy_product_api::InterceptProxyProfile;
use rusqlite::Connection;

use super::*;

#[derive(Debug)]
struct NoFileDialog;

impl NativeFileDialog for NoFileDialog {
    fn choose_open_file(&self, _purpose: &str) -> AppResult<Option<PathBuf>> {
        Ok(None)
    }

    fn choose_save_file(
        &self,
        _purpose: &str,
        _suggested_file_name: &str,
    ) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct TestSecretProtector;

impl SecretProtector for TestSecretProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(plaintext.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        self.protect(ciphertext)
    }
}

#[derive(Debug)]
struct RefusingSecretProtector;

impl SecretProtector for RefusingSecretProtector {
    fn protect(&self, _: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Err(InfrastructureError::KeychainProtect)
    }

    fn unprotect(&self, _: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Err(InfrastructureError::KeychainUnprotect)
    }
}

#[tokio::test]
async fn builds_and_invokes_application_without_tauri() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let host = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("build UI-neutral host");

    assert!(host.begin_shutdown(), "first caller owns graceful shutdown");
    assert!(
        !host.begin_shutdown(),
        "repeated callers must reuse the existing shutdown task"
    );
    assert!(!host.shutdown_completed());

    let application = host.application();
    let settings = application.settings_get().await.expect("query settings");
    assert_eq!(settings.stored.max_sessions, 500);
    assert_eq!(settings.stored.external_package_service.port, 8_765);
    let external_status = application
        .external_package_service_status()
        .await
        .expect("query external package service status");
    assert_eq!(external_status.fixed_path, "/packages");
    assert!(!external_status.authentication_enabled);

    let draft = application
        .rule_new_draft()
        .await
        .expect("create rule draft");
    assert_eq!(draft.name, "新建规则");

    host.shutdown().await.expect("shutdown UI-neutral host");
    assert!(host.shutdown_completed());
}

#[tokio::test]
async fn keychain_refusal_does_not_prevent_host_or_bootstrap_startup() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let host = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(RefusingSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("host startup must not access the system secret store");
    let application = host.application();

    let bootstrap = application
        .app_bootstrap()
        .await
        .expect("metadata-only bootstrap remains available");
    assert!(bootstrap.certificate.can_initialize);

    let error = application
        .certificate_initialize_if_needed()
        .await
        .expect_err("explicit certificate initialization reports refusal");
    assert_eq!(error.view_model.code, "KEYCHAIN_PROTECT_FAILED");

    host.shutdown().await.expect("shutdown UI-neutral host");
}

#[tokio::test]
async fn external_package_bind_failure_is_visible_without_blocking_host_startup() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve local port");
    let port = occupied.local_addr().expect("reserved address").port();
    let database = temp.path().join("intercept-proxy.sqlite3");
    let product = InterceptProxyProfile;
    let store = Arc::new(SqliteStore::open(&database).expect("open settings database"));
    let settings = SettingsRepositoryAdapter::new(store, &product);
    let mut draft = settings.defaults().await.expect("default settings");
    draft.external_package_service.bind_address = "127.0.0.1".into();
    draft.external_package_service.port = port;
    settings.save(draft).await.expect("persist occupied port");

    let host = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(product),
    )
    .build()
    .await
    .expect("external service bind failure must be non-fatal");
    let status = host
        .application()
        .external_package_service_status()
        .await
        .expect("query failed service status");

    assert!(matches!(
        status.state,
        ExternalPackageServiceStateViewModel::Failed { .. }
    ));
    assert!(status.websocket_url.ends_with(&format!(":{port}/packages")));
    host.shutdown().await.expect("shutdown host");
}

#[tokio::test]
async fn pre_1_0_database_is_deleted_and_recreated_from_current_defaults() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let database = temp.path().join("intercept-proxy.sqlite3");
    let connection = Connection::open(&database).expect("create pre-1.0 database");
    connection
        .execute_batch(
            "CREATE TABLE application_schema(
                 singleton_id INTEGER PRIMARY KEY,
                 version INTEGER NOT NULL
             );
             INSERT INTO application_schema(singleton_id, version) VALUES (1, 9);
             CREATE TABLE pre_1_0_sentinel(value TEXT NOT NULL);
             INSERT INTO pre_1_0_sentinel(value) VALUES ('must be deleted');",
        )
        .expect("write pre-1.0 schema");
    drop(connection);

    let host = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("pre-1.0 data is replaced by a fresh 1.0 database");

    let workspaces = host
        .application()
        .workspace_list()
        .await
        .expect("fresh workspace list");
    assert_eq!(workspaces.len(), 1);
    assert!(workspaces[0].selected);
    host.shutdown().await.expect("shutdown host");

    let connection = Connection::open(database).expect("open recreated database");
    let sentinel_exists: bool = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'pre_1_0_sentinel'
            )",
            [],
            |row| row.get(0),
        )
        .expect("query sentinel");
    assert!(!sentinel_exists);
}

#[tokio::test]
async fn unrelated_database_open_error_does_not_delete_the_file() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let database = temp.path().join("intercept-proxy.sqlite3");
    let original = b"not a sqlite database";
    std::fs::write(&database, original).expect("write invalid database bytes");

    let error = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect_err("ordinary database open errors are not reset automatically");
    assert!(matches!(error, HostBuildError::Infrastructure(_)));
    assert_eq!(
        std::fs::read(database).expect("read original file"),
        original
    );
}

#[tokio::test]
async fn newer_schema_is_not_deleted_by_the_1_0_reset_policy() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let database = temp.path().join("intercept-proxy.sqlite3");
    let connection = Connection::open(&database).expect("create newer database");
    connection
        .execute_batch(&format!(
            "CREATE TABLE application_schema(
                 singleton_id INTEGER PRIMARY KEY,
                 version INTEGER NOT NULL
             );
             INSERT INTO application_schema(singleton_id, version) VALUES (1, {});
             CREATE TABLE future_sentinel(value TEXT NOT NULL);",
            CURRENT_APPLICATION_SCHEMA_VERSION + 1
        ))
        .expect("write newer schema");
    drop(connection);

    ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect_err("newer schema must not be deleted as pre-1.0 data");

    let connection = Connection::open(database).expect("newer database still exists");
    let sentinel_exists: bool = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'future_sentinel'
            )",
            [],
            |row| row.get(0),
        )
        .expect("query future sentinel");
    assert!(sentinel_exists);
}
