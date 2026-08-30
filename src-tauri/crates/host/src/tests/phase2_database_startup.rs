use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::Arc,
};

use intercept_proxy_application::{
    AppResult, ConditionTree, MessageStage, RuleActionKind, RuleConditionKind, RuleContent,
    RuleEditorContentContext, RuleStage, UnifiedAction, WorkspaceId,
};
use intercept_proxy_infrastructure::{FileSelection, NativeFileDialog};
use intercept_proxy_product_api::InterceptProxyProfile;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;

const MANIFEST: &str = include_str!(
    "../../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SecondStartExpectation {
    Recreated,
    Preserved,
}

#[derive(Debug)]
struct TwoStartFixture {
    workspace_id: WorkspaceId,
    listener_id: intercept_proxy_application::ListenerId,
    rule: intercept_proxy_application::RuleDefinition,
}

#[tokio::test]
async fn explicit_recreate_policy_removes_current_schema_data_before_host_reads() {
    assert_two_start_database_contract(
        Some(DatabaseStartupPolicy::RecreateCurrent),
        SecondStartExpectation::Recreated,
    )
    .await;
}

#[tokio::test]
async fn default_preserve_policy_keeps_current_schema_data_across_host_starts() {
    assert_two_start_database_contract(None, SecondStartExpectation::Preserved).await;
}

#[tokio::test]
async fn recreate_policy_propagates_database_open_failure_without_starting_host() {
    let temp = tempfile::tempdir().expect("temporary host directory");
    std::fs::write(
        temp.path().join("intercept-proxy.sqlite3"),
        b"not a sqlite database",
    )
    .expect("write invalid database bytes");

    let error = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .with_database_startup_policy(DatabaseStartupPolicy::RecreateCurrent)
    .build()
    .await
    .expect_err("database failure must prevent Host construction");

    assert!(matches!(error, HostBuildError::Infrastructure(_)));
}

async fn assert_two_start_database_contract(
    second_start_policy: Option<DatabaseStartupPolicy>,
    expectation: SecondStartExpectation,
) {
    let temp = tempfile::tempdir().expect("temporary two-start host directory");
    let package_zip = temp.path().join("strict-javascript-package.zip");
    std::fs::write(&package_zip, javascript_package_zip())
        .expect("write strict JavaScript package ZIP");
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
    let fixture = seed_two_start_fixture(&first).await;
    assert_two_start_fixture_present(first.application().as_ref(), &fixture).await;
    first.shutdown().await.expect("shutdown first Host");

    let mut second_builder = ApplicationHostBuilder::new(
        temp.path(),
        HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    );
    if let Some(policy) = second_start_policy {
        second_builder = second_builder.with_database_startup_policy(policy);
    }
    let second = second_builder.build().await.expect("second Host startup");
    match expectation {
        SecondStartExpectation::Recreated => {
            assert_two_start_fixture_absent(second.application().as_ref(), &fixture).await;
        }
        SecondStartExpectation::Preserved => {
            assert_two_start_fixture_present(second.application().as_ref(), &fixture).await;
        }
    }
    second.shutdown().await.expect("shutdown second Host");
}

async fn seed_two_start_fixture(host: &ApplicationHost) -> TwoStartFixture {
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
    let mut input = stages
        .into_iter()
        .find(|stage| stage.stage == RuleStage::ProxyToUpstream)
        .expect("proxy-to-upstream stage")
        .new_rule_draft;
    input.draft.name = "phase2-two-start-rule".into();
    input.draft.priority = 29;
    let RuleContent::Http(content) = &mut input.draft.content else {
        panic!("HTTP rule draft expected");
    };
    content.description = "phase2 lifecycle fixture".into();
    content.condition = ConditionTree::Leaf(
        application
            .rule_definition_condition_draft(RuleConditionKind::NthHit, MessageStage::Request)
            .expect("current NthHit condition"),
    );
    content.actions = vec![UnifiedAction::from(
        application
            .rule_definition_action_draft(RuleActionKind::Delay, MessageStage::Request)
            .expect("current delay action"),
    )];
    input.draft.one_shot = true;
    let created = application
        .rule_definition_save(input)
        .await
        .expect("save Rule through Application");
    let toggled = application
        .rule_definition_toggle(created.rule_id(), created.revision(), false)
        .await
        .expect("advance Rule revision through Application");

    let import_error = application
        .protocol_package_import()
        .await
        .expect_err("strict JavaScript ZIP stops at the Phase 8 runtime boundary");
    assert_eq!(import_error.view_model.code, "PROTOCOL_PACKAGE_INVALID");
    assert!(import_error.view_model.field_errors.contains_key("runtime"));
    assert!(
        application
            .protocol_package_list()
            .await
            .expect("list Packages after Phase 8 fail-closed import")
            .is_empty(),
        "Phase 7 must not persist a package before JavaScript compilation exists"
    );

    TwoStartFixture {
        workspace_id: workspace.id,
        listener_id: listener.id,
        rule: toggled,
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
    assert_eq!(rule.revision(), fixture.rule.revision());
    assert!(!rule.enabled());
    assert!(rule.one_shot());
    assert_eq!(rule.lifecycle().hit_count, 0);
    assert_eq!(rule.lifecycle().last_hit_at, None);
    assert!(
        application
            .protocol_package_list()
            .await
            .expect("list Packages after preserved Host restart")
            .is_empty(),
        "Phase 8 has not run, so no package may have been persisted"
    );
}

async fn assert_two_start_fixture_absent(
    application: &intercept_proxy_application::Application,
    fixture: &TwoStartFixture,
) {
    let workspaces = application
        .workspace_list()
        .await
        .expect("list Workspaces after recreate");
    assert!(
        workspaces
            .iter()
            .all(|workspace| workspace.id != fixture.workspace_id),
        "the unique Workspace must be absent; its Listener and Rule are owned by that aggregate"
    );

    assert!(
        application
            .protocol_package_list()
            .await
            .expect("list Packages after recreate")
            .is_empty(),
        "Phase 8 has not run, so no package may have been persisted"
    );
}

fn javascript_package_zip() -> Vec<u8> {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.json", MANIFEST.as_bytes()),
        ("protocol.js", b"export {}".as_slice()),
        ("display.js", b"export {}".as_slice()),
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
