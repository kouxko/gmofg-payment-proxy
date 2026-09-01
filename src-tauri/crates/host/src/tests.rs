use std::path::PathBuf;

use intercept_proxy_application::{AppResult, ExternalPackageServiceStateViewModel};
use intercept_proxy_infrastructure::{
    CURRENT_APPLICATION_SCHEMA_VERSION, FileSelection, InfrastructureError, NativeFileDialog,
    SecretProtector, SqliteStore,
};
use intercept_proxy_product_api::InterceptProxyProfile;
use rusqlite::Connection;

use super::*;

#[path = "tests/phase2_database_startup.rs"]
mod phase2_database_startup;

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

    let selected = application
        .workspace_list()
        .await
        .expect("workspace list")
        .into_iter()
        .find(|workspace| workspace.selected)
        .expect("selected workspace");
    let listener_id = application
        .workspace_get(selected.id)
        .await
        .expect("workspace detail")
        .listeners[0]
        .id;
    let context = application
        .rule_editor_context(listener_id)
        .await
        .expect("load unified HTTP rule context");
    let intercept_proxy_application::RuleEditorContentContext::Http { stages } = context.content
    else {
        panic!("HTTP rule context expected");
    };
    let draft = &stages[0].new_rule_draft;
    assert_eq!(draft.listener_id, listener_id);
    assert_eq!(
        draft.stage,
        intercept_proxy_application::RuleStage::ProxyToUpstream
    );
    let intercept_proxy_application::RuleContent::Http(http_draft_content) = &draft.content else {
        panic!("HTTP structural draft expected");
    };
    assert!(http_draft_content.conditions.is_empty());
    assert!(http_draft_content.actions.is_empty());

    host.shutdown().await.expect("shutdown UI-neutral host");
    assert!(host.shutdown_completed());
}

#[tokio::test(flavor = "current_thread")]
async fn host_build_keeps_current_thread_progressing_while_schema_open_waits() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let database = temp.path().join("intercept-proxy.sqlite3");
    drop(SqliteStore::open(&database).expect("initialize current schema"));
    let blocker = Connection::open(&database).expect("open schema blocker");
    blocker
        .execute_batch("PRAGMA busy_timeout = 5000; BEGIN EXCLUSIVE;")
        .expect("hold writer lock during Host bootstrap");

    let build = tokio::spawn({
        let data_dir = temp.path().to_path_buf();
        async move {
            ApplicationHostBuilder::new(
                data_dir,
                HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
                Arc::new(InterceptProxyProfile),
            )
            .build()
            .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_millis(250), async {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    })
    .await
    .expect("Host SQLite bootstrap did not block the current-thread runtime");
    assert!(
        !build.is_finished(),
        "schema open must still be waiting for lock"
    );

    blocker
        .execute_batch("COMMIT;")
        .expect("release schema lock");
    let host = build
        .await
        .expect("Host build task joined")
        .expect("Host build completed after schema lock release");
    host.shutdown().await.expect("shutdown Host");
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
    let initial = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("build host before changing persisted settings");

    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve local port");
    let port = occupied.local_addr().expect("reserved address").port();
    let application = initial.application();
    let mut draft = application
        .settings_get()
        .await
        .expect("load settings through Application")
        .stored;
    draft.external_package_service.bind_address = "127.0.0.1".into();
    draft.external_package_service.port = port;
    application
        .settings_save(draft)
        .await
        .expect("persist occupied port through Application");
    initial.shutdown().await.expect("shutdown initial host");

    let host = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
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
async fn pre_1_0_schema_is_cleared_and_recreated_as_schema100() {
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
    assert_eq!(CURRENT_APPLICATION_SCHEMA_VERSION, 100);
    let host = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("pre-1.0 schema must recreate current storage");
    host.shutdown().await.expect("shutdown recreated Host");

    let connection = Connection::open(&database).expect("open recreated database");
    let version = connection
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("Schema100 marker");
    let sentinel_exists = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_schema
                WHERE type = 'table' AND name = 'pre_1_0_sentinel'
            )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("legacy sentinel probe");
    assert_eq!(version, 100);
    assert!(!sentinel_exists, "pre-1.0 data must be deleted");
}

#[tokio::test]
async fn missing_schema_marker_is_rejected_without_changing_any_sqlite_file() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let database = temp.path().join("intercept-proxy.sqlite3");
    let connection = Connection::open(&database).expect("create markerless database");
    connection
        .execute_batch(
            "CREATE TABLE markerless_sentinel(value TEXT NOT NULL);
             INSERT INTO markerless_sentinel(value) VALUES ('must remain');",
        )
        .expect("write markerless database");
    drop(connection);
    let before = sqlite_files(&database);

    let error = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect_err("missing schema marker must fail closed");

    assert!(matches!(
        error,
        HostBuildError::Infrastructure(InfrastructureError::DatabaseSchemaInvalid { .. })
    ));
    assert_sqlite_files_unchanged(&database, &before);
}

#[tokio::test]
async fn unrelated_database_open_error_does_not_delete_the_file() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let database = temp.path().join("intercept-proxy.sqlite3");
    let original = b"not a sqlite database";
    std::fs::write(&database, original).expect("write invalid database bytes");

    let before = sqlite_files(&database);
    let error = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect_err("ordinary database open errors are not reset automatically");
    assert!(matches!(error, HostBuildError::Infrastructure(_)));
    assert_sqlite_files_unchanged(&database, &before);
}

#[tokio::test]
async fn newer_schema_is_rejected_without_changing_any_sqlite_file() {
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
    let before = sqlite_files(&database);

    let error = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect_err("newer schema must fail closed");
    assert!(matches!(
        error,
        HostBuildError::Infrastructure(InfrastructureError::DatabaseSchemaInvalid { .. })
    ));
    assert_sqlite_files_unchanged(&database, &before);
}

#[tokio::test]
async fn incompatible_workspace_record_is_rejected_before_any_sqlite_write() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    let database = temp.path().join("intercept-proxy.sqlite3");
    drop(SqliteStore::open(&database).expect("initialize current schema"));
    let workspace_id = uuid::Uuid::from_u128(0x50_4552_5349_5354_454e_4345);
    let connection = Connection::open(&database).expect("open current database");
    connection
        .execute(
            "INSERT INTO workspaces(id, revision, json, updated_at) VALUES (?1, 1, ?2, ?3)",
            rusqlite::params![
                workspace_id.to_string(),
                r#"{"_persistence_version": 6, "rules": []}"#,
                "2026-08-28T00:00:00Z"
            ],
        )
        .expect("insert incompatible workspace record");
    connection
        .execute(
            "UPDATE workspace_state SET selected_id = ?1 WHERE singleton_id = 1",
            [workspace_id.to_string()],
        )
        .expect("select incompatible workspace record");
    drop(connection);
    let before = sqlite_files(&database);

    let error = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect_err("incompatible persisted record must fail closed");
    let HostBuildError::Application(error) = error else {
        panic!("incompatible workspace must retain the stable Application error boundary");
    };
    assert_eq!(error.view_model.code, "PERSISTENCE_CORRUPT");
    assert_sqlite_files_unchanged(&database, &before);
}

#[derive(Debug, Eq, PartialEq)]
struct FileSnapshot {
    bytes: Option<Vec<u8>>,
    modified: Option<std::time::SystemTime>,
}

#[derive(Debug, Eq, PartialEq)]
struct SqliteFiles {
    database: FileSnapshot,
    wal: FileSnapshot,
    shm: FileSnapshot,
}

fn sqlite_files(database: &std::path::Path) -> SqliteFiles {
    let sidecar = |suffix: &str| {
        let mut path = database.as_os_str().to_owned();
        path.push(suffix);
        file_snapshot(&PathBuf::from(path))
    };
    SqliteFiles {
        database: file_snapshot(database),
        wal: sidecar("-wal"),
        shm: sidecar("-shm"),
    }
}

fn file_snapshot(path: &std::path::Path) -> FileSnapshot {
    FileSnapshot {
        bytes: std::fs::read(path).ok(),
        modified: std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok(),
    }
}

fn assert_sqlite_files_unchanged(database: &std::path::Path, before: &SqliteFiles) {
    let after = sqlite_files(database);
    assert!(
        after == *before,
        "database, WAL, or SHM existence/content/mtime changed"
    );
}
