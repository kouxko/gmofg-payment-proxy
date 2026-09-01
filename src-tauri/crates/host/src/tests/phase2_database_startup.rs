use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::Arc,
};

use intercept_proxy_application::{
    AppResult, ProtocolPackageKindViewModel, ProtocolPackageValidationViewModel, RuleActionKind,
    RuleContent, RuleEditorContentContext, RuleStage, UnifiedAction, WorkspaceId,
};
use intercept_proxy_infrastructure::{FileSelection, NativeFileDialog};
use intercept_proxy_product_api::InterceptProxyProfile;
use rusqlite::OptionalExtension;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;

const MANIFEST: &str = include_str!(
    "../../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
);
const PROTOCOL_JS: &[u8] = include_bytes!(
    "../../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/protocol.js"
);
const DISPLAY_JS: &[u8] = include_bytes!(
    "../../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/display.js"
);

#[derive(Debug)]
struct StaticOpenDialog(PathBuf);

impl NativeFileDialog for StaticOpenDialog {
    fn choose_open_file(&self, purpose: &str) -> AppResult<Option<PathBuf>> {
        assert_eq!(purpose, "protocol_package_zip");
        Ok(Some(self.0.clone()))
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
struct TwoStartFixture {
    workspace_id: WorkspaceId,
    listener_id: intercept_proxy_application::ListenerId,
    rule: intercept_proxy_application::RuleDefinition,
    package: intercept_proxy_application::ProtocolPackageRef,
    package_archive: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
struct StoredPackageSnapshot {
    registration_json: String,
    registration_fingerprint: Vec<u8>,
    local_archive: Vec<u8>,
    enabled: i64,
    first_connected_at: String,
    last_connected_at: String,
    last_remote_address: Option<String>,
    recent_error_code: Option<String>,
    recent_error_message: Option<String>,
    recent_error_occurred_at: Option<String>,
}

#[tokio::test]
async fn release_startup_preserves_schema100_state_across_two_real_host_starts() {
    assert_two_start_database_contract().await;
}

async fn assert_two_start_database_contract() {
    let temp = tempfile::tempdir().expect("temporary two-start host directory");
    let package_zip = temp.path().join("strict-javascript-package.zip");
    let package_archive = javascript_package_zip();
    std::fs::write(&package_zip, &package_archive).expect("write strict JavaScript package ZIP");
    let first = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(
            Arc::new(TestSecretProtector),
            Arc::new(StaticOpenDialog(package_zip)),
        ),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("first Host startup preserves an empty database");
    let fixture = seed_two_start_fixture(&first, package_archive).await;
    assert_two_start_fixture_present(first.application().as_ref(), &fixture).await;
    first.shutdown().await.expect("shutdown first Host");
    let first_package = load_package_snapshot(temp.path(), &fixture)
        .expect("local package row after first shutdown");
    assert_package_snapshot_contract(&first_package, &fixture);
    assert_schema100_without_legacy_tables(temp.path());

    let second = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("second Release-equivalent Host startup");
    assert_two_start_fixture_present(second.application().as_ref(), &fixture).await;
    let second_package = load_package_snapshot(temp.path(), &fixture)
        .expect("local package row after second startup");
    assert_eq!(
        second_package.registration_json,
        first_package.registration_json
    );
    assert_eq!(
        second_package.registration_fingerprint,
        first_package.registration_fingerprint
    );
    assert_eq!(second_package.local_archive, first_package.local_archive);
    assert_eq!(second_package.enabled, first_package.enabled);
    assert_eq!(
        second_package.first_connected_at,
        first_package.first_connected_at
    );
    assert_eq!(
        second_package.last_connected_at,
        first_package.last_connected_at
    );
    assert_eq!(second_package.last_remote_address, None);
    assert_eq!(
        second_package.recent_error_code,
        first_package.recent_error_code
    );
    assert_eq!(
        second_package.recent_error_message,
        first_package.recent_error_message
    );
    assert!(second_package.recent_error_occurred_at >= first_package.recent_error_occurred_at);
    assert_schema100_without_legacy_tables(temp.path());
    second.shutdown().await.expect("shutdown second Host");
}

async fn seed_two_start_fixture(
    host: &ApplicationHost,
    package_archive: Vec<u8>,
) -> TwoStartFixture {
    let application = host.application();
    let workspace = application
        .workspace_create("phase2-two-start-workspace".into())
        .await
        .expect("create unique Workspace through Application");
    application
        .workspace_select(workspace.id)
        .await
        .expect("select unique Workspace through Application");
    let listener = workspace.listeners[0].clone();
    assert!(!listener.enabled, "fixture Listener is explicitly disabled");
    let RuleEditorContentContext::Http { stages } = application
        .rule_editor_context(listener.id)
        .await
        .expect("load current HTTP rule editor contract")
        .content
    else {
        panic!("HTTP rule context expected");
    };
    let structure = stages
        .into_iter()
        .find(|stage| stage.stage == RuleStage::ProxyToUpstream)
        .expect("proxy-to-upstream stage")
        .new_rule_draft;
    let intercept_proxy_application::RuleNewDefinitionDraft {
        listener_id,
        stage,
        content,
    } = structure;
    let intercept_proxy_application::RuleNewDefinitionContent::Http { .. } = content else {
        panic!("HTTP rule draft expected");
    };
    let description = "phase2 lifecycle fixture".into();
    let condition = application
            .rule_definition_http_condition_draft(
                intercept_proxy_application::RuleMatchFieldKind::Method,
                None,
                intercept_proxy_application::RuleMatchOperatorKind::Equals,
                "GET",
                stage,
            )
            .expect("method condition");
    let action = UnifiedAction::from(application
            .rule_definition_action_draft(
                intercept_proxy_application::RuleHttpActionDraftInput {
                    kind: RuleActionKind::Delay,
                    parameters_json: Some(r#"{"milliseconds":100}"#.into()),
                },
                RuleStage::ProxyToUpstream,
            )
            .expect("explicit delay action"));
    let input = intercept_proxy_application::RuleDefinitionSaveInput {
        rule_id: None,
        expected_revision: None,
        draft: intercept_proxy_application::RuleDefinitionDraft {
            name: "phase2-two-start-rule".into(),
            enabled: true,
            priority: 29,
            listener_id,
            stage,
            content: RuleContent::Http(intercept_proxy_application::HttpRuleContent {
                description,
                condition,
                action,
            }),
        },
    };
    let created = application
        .rule_definition_save(input)
        .await
        .expect("save Rule through Application");
    let toggled = application
        .rule_definition_toggle(created.rule_id(), created.revision(), false)
        .await
        .expect("advance Rule revision through Application");

    let preview = application
        .protocol_package_import()
        .await
        .expect("prepare strict JavaScript ZIP")
        .expect("native dialog selected a ZIP");
    let package = preview.package.clone();
    let imported = application
        .protocol_package_import_commit(preview.token.expect("committable package"))
        .await
        .expect("commit validated local ZIP to the unified registry");
    assert_eq!(imported.version.package, package);
    assert!(imported.version.enabled);
    assert_eq!(imported.version.source.external_online(), Some(false));
    assert_package_persisted(application.as_ref(), &package).await;

    TwoStartFixture {
        workspace_id: workspace.id,
        listener_id: listener.id,
        rule: toggled,
        package,
        package_archive,
    }
}

async fn assert_two_start_fixture_present(
    application: &intercept_proxy_application::Application,
    fixture: &TwoStartFixture,
) {
    let workspace = application
        .workspace_get(fixture.workspace_id)
        .await
        .expect("unique Workspace is present");
    let listener = workspace
        .listeners
        .iter()
        .find(|listener| listener.id == fixture.listener_id)
        .expect("unique Listener is present");
    assert!(!listener.enabled);
    application
        .workspace_select(fixture.workspace_id)
        .await
        .expect("select fixture Workspace");
    let rule = application
        .rule_definition_get(fixture.rule.rule_id())
        .await
        .expect("unique Rule is present");
    assert_eq!(rule, fixture.rule);
    assert_eq!(rule.revision(), fixture.rule.revision());
    assert!(!rule.enabled());
    assert_eq!(rule.lifecycle().hit_count, 0);
    assert_eq!(rule.lifecycle().last_hit_at, None);
    assert_package_persisted(application, &fixture.package).await;
}

async fn assert_package_persisted(
    application: &intercept_proxy_application::Application,
    package: &intercept_proxy_application::ProtocolPackageRef,
) {
    let groups = application
        .protocol_package_list()
        .await
        .expect("list persisted package");
    let version = groups
        .iter()
        .flat_map(|group| &group.versions)
        .find(|version| &version.package == package)
        .expect("exact local ZIP package persists");
    assert!(version.enabled);
    assert_eq!(version.source.external_online(), Some(false));
    assert_eq!(version.name, "Payment JSON");
    assert_eq!(version.host_api, 1);
    assert_eq!(version.kind, ProtocolPackageKindViewModel::Http);
    assert_eq!(
        version.validation,
        ProtocolPackageValidationViewModel::Valid
    );

    let detail = application
        .protocol_package_detail(package.clone())
        .await
        .expect("load persisted package detail");
    let external = detail.external.expect("external package lifecycle detail");
    assert!(external.local_process);
    assert_eq!(external.remote_address, None);
    let recent_error = external.recent_error.expect("stable local process failure");
    assert_eq!(recent_error.code, "EXTERNAL_PACKAGE_PROCESS_FAILED");
    assert_eq!(recent_error.message, "本地软件包进程启动失败。");
}

fn load_package_snapshot(
    data_dir: &std::path::Path,
    fixture: &TwoStartFixture,
) -> Option<StoredPackageSnapshot> {
    let connection = rusqlite::Connection::open(data_dir.join("intercept-proxy.sqlite3"))
        .expect("open Host database readback");
    connection
        .query_row(
            "SELECT registration_json, registration_fingerprint, local_archive, enabled,
                    first_connected_at, last_connected_at, last_remote_address,
                    recent_error_code, recent_error_message, recent_error_occurred_at
             FROM external_protocol_packages WHERE package_id = ?1 AND version = ?2",
            rusqlite::params![
                fixture.package.id.as_str(),
                fixture.package.version.as_str()
            ],
            |row| {
                Ok(StoredPackageSnapshot {
                    registration_json: row.get(0)?,
                    registration_fingerprint: row.get(1)?,
                    local_archive: row.get(2)?,
                    enabled: row.get(3)?,
                    first_connected_at: row.get(4)?,
                    last_connected_at: row.get(5)?,
                    last_remote_address: row.get(6)?,
                    recent_error_code: row.get(7)?,
                    recent_error_message: row.get(8)?,
                    recent_error_occurred_at: row.get(9)?,
                })
            },
        )
        .optional()
        .expect("read exact local package row")
}

fn assert_package_snapshot_contract(snapshot: &StoredPackageSnapshot, fixture: &TwoStartFixture) {
    let registration: serde_json::Value =
        serde_json::from_str(&snapshot.registration_json).expect("stored registration JSON");
    let expected: serde_json::Value =
        serde_json::from_str(MANIFEST).expect("fixture Manifest JSON");
    assert_eq!(registration, expected);
    assert_eq!(snapshot.registration_fingerprint.len(), 32);
    assert_eq!(snapshot.local_archive, fixture.package_archive);
    assert_eq!(snapshot.enabled, 1);
    assert_eq!(snapshot.first_connected_at, snapshot.last_connected_at);
    assert_eq!(snapshot.last_remote_address, None);
    assert_eq!(
        snapshot.recent_error_code.as_deref(),
        Some("EXTERNAL_PACKAGE_PROCESS_FAILED")
    );
    assert_eq!(
        snapshot.recent_error_message.as_deref(),
        Some("本地软件包进程启动失败。")
    );
    assert!(snapshot.recent_error_occurred_at.is_some());
}

fn assert_schema100_without_legacy_tables(data_dir: &std::path::Path) {
    let connection = rusqlite::Connection::open(data_dir.join("intercept-proxy.sqlite3"))
        .expect("open Host database schema readback");
    let version = connection
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("read Schema100 marker");
    assert_eq!(version, 100);
    for table in [
        "protocol_packages",
        "protocol_package_files",
        "protocol_document_rules",
        "pre_1_0_sentinel",
        "pre_baseline_sentinel",
    ] {
        let exists = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get::<_, bool>(0),
            )
            .expect("probe legacy table");
        assert!(!exists, "legacy table {table} must not exist");
    }
}

fn javascript_package_zip() -> Vec<u8> {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.json", MANIFEST.as_bytes()),
        ("protocol.js", PROTOCOL_JS),
        ("display.js", DISPLAY_JS),
    ] {
        archive
            .start_file(path, SimpleFileOptions::default())
            .expect("start strict JavaScript package entry");
        archive
            .write_all(contents)
            .expect("write strict JavaScript package entry");
    }
    archive
        .finish()
        .expect("finish strict JavaScript package ZIP")
        .into_inner()
}
