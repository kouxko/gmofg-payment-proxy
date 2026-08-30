use super::*;

#[tokio::test]
async fn new_rule_defaults_are_owned_by_the_rust_repository() {
    let draft = RuleRepositoryAdapter::new_http_draft(test_channel());
    assert_eq!(draft.name, "新建规则");
    assert_eq!(draft.priority, 100);
    assert_eq!(draft.stage, Some(AppMessageStage::Request));
    assert!(draft.rule_id.is_none());
    assert!(draft.expected_revision.is_none());
    assert!(draft.conditions.is_empty());
    assert!(draft.actions.is_empty());
    assert_eq!(draft.channel, Some(test_channel()));
}

#[tokio::test]
async fn json_action_validation_returns_a_chinese_field_error() {
    let adapter = adapter();
    let mut empty = request_delay_draft("设置 JSON 字段", false);
    empty.actions = vec![AppRuleAction::SetJsonField {
        path: "$.result".into(),
        value_json: String::new(),
    }];

    let empty_validation = adapter.validate(&empty).await.expect("validate empty JSON");
    assert!(!empty_validation.valid);
    assert_eq!(
        empty_validation.field_errors["actions.0.value_json"],
        vec!["动作 1 的 JSON 值不能为空；请输入 null、字符串、数字、对象或数组"]
    );

    empty.actions = vec![AppRuleAction::SetJsonField {
        path: "$.result".into(),
        value_json: "{".into(),
    }];
    let malformed_validation = adapter
        .validate(&empty)
        .await
        .expect("validate malformed JSON");
    let message = &malformed_validation.field_errors["actions.0.value_json"][0];
    assert!(message.starts_with("动作 1 的 JSON 值格式无效；"));
    assert!(!message.contains("expected value"));
}

// RULE-003, RULE-011, ENGINE-008, TEST-RULE
#[tokio::test]
async fn domain_validation_and_sqlite_revision_are_enforced() {
    let adapter = adapter();
    let created = adapter
        .save(request_delay_draft("延迟", false))
        .await
        .expect("create");
    assert_eq!(created.summary.revision, 1);
    assert_eq!(adapter.list().await.expect("list").len(), 1);

    let mut stale = created.draft;
    stale.expected_revision = Some(0);
    assert_eq!(
        adapter
            .save(stale)
            .await
            .expect_err("stale")
            .view_model
            .code,
        "REVISION_CONFLICT"
    );
}

#[tokio::test]
async fn multiple_new_rules_and_toggle_are_persisted_independently() {
    let adapter = adapter();
    let first = adapter
        .save(request_delay_draft("规则一", false))
        .await
        .expect("create first rule");
    let second = adapter
        .save(request_delay_draft("规则二", true))
        .await
        .expect("create second rule");

    let listed = adapter.list().await.expect("list both rules");
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|rule| rule.name == "规则一"));
    assert!(listed.iter().any(|rule| rule.name == "规则二"));

    adapter
        .toggle(first.summary.rule_id, first.summary.revision, false)
        .await
        .expect("disable first rule");

    let listed = adapter.list().await.expect("list after toggle");
    assert_eq!(listed.len(), 2);
    assert!(
        !listed
            .iter()
            .find(|rule| rule.rule_id == first.summary.rule_id)
            .expect("first rule remains")
            .enabled
    );
    assert!(
        listed
            .iter()
            .find(|rule| rule.rule_id == second.summary.rule_id)
            .expect("second rule remains")
            .enabled
    );
}

#[tokio::test]
async fn selected_workspace_owns_rule_list_runtime_snapshot_and_revision() {
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    let first_workspace = seed_workspace(&store, Vec::new());
    let adapter = adapter_with(Arc::clone(&store), Arc::new(NoDialog));
    adapter
        .save(request_delay_draft("first workspace rule", false))
        .await
        .expect("save first rule");
    let first_after_save = store
        .load_workspaces()
        .expect("workspaces")
        .records
        .into_iter()
        .find(|record| record.id == first_workspace.id.as_uuid())
        .expect("first workspace");
    assert_eq!(
        first_after_save.revision,
        first_workspace.revision.get() + 1
    );

    let second_rule = Rule::create(
        to_domain_draft(&request_delay_draft("second workspace rule", false), 1)
            .expect("second draft"),
    )
    .expect("second rule");
    let second_workspace = seed_workspace(&store, vec![second_rule.clone()]);
    store
        .select_workspace(second_workspace.id.as_uuid())
        .expect("select second workspace");

    let listed = adapter.list().await.expect("selected rules");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "second workspace rule");
    let runtime = runtime_snapshot(&adapter).await;
    assert_eq!(runtime.collection_revision, second_workspace.revision.get());
    assert_eq!(
        runtime.rules,
        second_workspace.http_runtime_rules().unwrap()
    );

    store
        .select_workspace(first_workspace.id.as_uuid())
        .expect("reselect first workspace");
    assert_eq!(
        adapter.list().await.expect("first rules")[0].name,
        "first workspace rule"
    );
}

#[tokio::test]
async fn runtime_snapshot_commit_stays_bound_to_owning_workspace_after_ui_switch() {
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    let first = seed_workspace(&store, Vec::new());
    let adapter = adapter_with(Arc::clone(&store), Arc::new(NoDialog));
    adapter
        .save(request_delay_draft("shared rule", false))
        .await
        .expect("save rule");
    let stale = runtime_snapshot(&adapter).await;

    let second = ProxyWorkspace {
        revision: Revision::new(stale.collection_revision),
        ..ProxyWorkspace::default()
    };
    store
        .insert_workspace(&RuleRepositoryAdapter::workspace_record(&second).expect("record"))
        .expect("insert identical workspace");
    store
        .select_workspace(second.id.as_uuid())
        .expect("switch workspace");

    let revision = adapter
        .commit_runtime_deltas(&stale, &[])
        .await
        .expect("runtime commit remains on first workspace");
    assert_eq!(revision, stale.collection_revision + 1);
    let snapshot = store.load_workspaces().expect("workspaces");
    assert_eq!(snapshot.selected_id, Some(second.id.as_uuid()));
    assert_eq!(
        snapshot
            .records
            .iter()
            .find(|record| record.id == first.id.as_uuid())
            .expect("first workspace")
            .revision,
        revision
    );
}

#[test]
fn application_rule_write_rejects_workspace_that_is_no_longer_selected() {
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    let first = seed_workspace(&store, Vec::new());
    let second = seed_workspace(&store, Vec::new());
    store
        .select_workspace(first.id.as_uuid())
        .expect("select first workspace");

    let mut edited_first = first.clone();
    let mut edited_rules = edited_first.http_runtime_rules().unwrap();
    edited_rules.push(
        Rule::create(
            to_domain_draft(&request_delay_draft("stale editor rule", false), 1).expect("draft"),
        )
        .expect("rule"),
    );
    edited_first
        .replace_http_runtime_rules(edited_rules)
        .unwrap();
    store
        .select_workspace(second.id.as_uuid())
        .expect("switch selected workspace");

    let error = RuleRepositoryAdapter::save_selected_workspace_to(
        &store,
        edited_first,
        first.revision.get(),
    )
    .expect_err("application write must stay bound to current selection");
    assert_eq!(error.view_model.code, "REVISION_CONFLICT");

    let snapshot = store.load_workspaces().expect("workspaces");
    assert_eq!(snapshot.selected_id, Some(second.id.as_uuid()));
    let persisted_first = snapshot
        .records
        .into_iter()
        .find(|record| record.id == first.id.as_uuid())
        .expect("first workspace");
    assert_eq!(persisted_first.revision, first.revision.get());
}
