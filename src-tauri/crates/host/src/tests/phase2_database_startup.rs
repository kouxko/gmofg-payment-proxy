use std::{path::PathBuf, sync::Arc};

use chrono::{TimeZone, Utc};
use intercept_proxy_application::{
    AppResult, MessageStage, ProtocolPackageRef, RuleActionKind, RuleContent,
    RuleEditorContentContext, RuleStage, WorkspaceId,
};
use intercept_proxy_infrastructure::{FileSelection, NativeFileDialog};
use intercept_proxy_product_api::InterceptProxyProfile;

use super::*;

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
    package: ProtocolPackageRef,
    last_hit_at: chrono::DateTime<Utc>,
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
    let package_zip = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../../examples/protocol-packages/nuvei_tango_rhai/dist/nuvei-tango-json-rhai-1.0.0.zip",
    );
    assert!(package_zip.is_file(), "tracked protocol package fixture");
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
    let last_hit_at = Utc
        .with_ymd_and_hms(2026, 8, 30, 3, 4, 5)
        .single()
        .expect("fixed lifecycle timestamp");
    let RuleContent::Http(content) = &mut input.draft.content else {
        panic!("HTTP rule draft expected");
    };
    content.description = "phase2 lifecycle fixture".into();
    content.one_shot = true;
    content.hit_count = 7;
    content.last_hit_at = Some(last_hit_at);
    content.actions = vec![
        application
            .rule_definition_action_draft(RuleActionKind::Delay, MessageStage::Request)
            .expect("current delay action"),
    ];
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
        .expect("prepare tracked ZIP through Application")
        .expect("native dialog selected tracked ZIP");
    let token = preview.token.expect("new package has commit token");
    let imported = application
        .protocol_package_import_commit(token)
        .await
        .expect("commit tracked ZIP through Application");

    TwoStartFixture {
        workspace_id: workspace.id,
        listener_id: listener.id,
        rule: toggled,
        package: imported.version.package,
        last_hit_at,
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
    let RuleContent::Http(content) = rule.content() else {
        panic!("HTTP rule expected");
    };
    assert!(content.one_shot);
    assert_eq!(content.hit_count, 7);
    assert_eq!(content.last_hit_at, Some(fixture.last_hit_at));
    let package = application
        .protocol_package_detail(fixture.package.clone())
        .await
        .expect("unique imported Package is present");
    assert_eq!(package.version.package, fixture.package);
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

    let packages = application
        .protocol_package_list()
        .await
        .expect("list Packages after recreate");
    assert!(
        packages.iter().all(|group| {
            group
                .versions
                .iter()
                .all(|version| version.package != fixture.package)
        }),
        "the exact imported Package identity must be absent"
    );
}
