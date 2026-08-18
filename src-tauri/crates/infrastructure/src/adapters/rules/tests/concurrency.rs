use super::*;

#[tokio::test]
async fn concurrent_same_revision_save_has_exactly_one_winner() {
    let adapter = adapter();
    let created = adapter
        .save(request_delay_draft("原始", false))
        .await
        .expect("create");
    let mut first = created.draft.clone();
    first.name = "first".into();
    let mut second = created.draft;
    second.name = "second".into();

    let (first, second) = tokio::join!(adapter.save(first), adapter.save(second));
    assert_ne!(first.is_ok(), second.is_ok());
    let conflict = first.err().or_else(|| second.err()).expect("one conflict");
    assert_eq!(conflict.view_model.code, "REVISION_CONFLICT");
    let stored = adapter
        .get(created.summary.rule_id)
        .await
        .expect("stored winner");
    assert_eq!(stored.summary.revision, 2);
}

#[tokio::test]
async fn import_rejects_cross_process_changes_instead_of_replacing_them() {
    let directory = tempfile::tempdir().expect("temp directory");
    let database = directory.path().join("rules.sqlite3");
    let import = directory.path().join("rules.json");
    std::fs::write(&import, b"[]").expect("import file");
    let primary_store = Arc::new(SqliteStore::open(&database).expect("primary store"));
    let secondary_store = Arc::new(SqliteStore::open(&database).expect("secondary store"));

    let existing = Rule::create(
        to_domain_draft(&request_delay_draft("existing", false), 1).expect("existing draft"),
    )
    .expect("existing rule");
    seed_workspace(&primary_store, vec![existing.clone()]);
    let concurrent = Rule::create(
        to_domain_draft(&request_delay_draft("concurrent", false), 2).expect("concurrent draft"),
    )
    .expect("concurrent rule");
    let adapter = RuleRepositoryAdapter::new(
        Arc::clone(&primary_store),
        Arc::new(MutatingOpenDialog {
            path: import,
            store: secondary_store,
            concurrent_rule: concurrent.clone(),
        }),
        Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
        &[],
    );

    let error = adapter.import().await.expect_err("stale import");
    assert_eq!(error.view_model.code, "REVISION_CONFLICT");
    let stored = RuleRepositoryAdapter::new(
        primary_store,
        Arc::new(NoDialog),
        Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
        &[],
    )
    .load()
    .expect("stored rules");
    assert_eq!(stored.len(), 2);
    assert!(stored.iter().any(|rule| rule.id == existing.id));
    assert!(stored.iter().any(|rule| rule.id == concurrent.id));
}

#[tokio::test]
async fn rule_import_rejects_files_over_the_rule_specific_limit() {
    let directory = tempfile::tempdir().expect("temp directory");
    let import = directory.path().join("oversized-rules.json");
    std::fs::File::create(&import)
        .expect("create import")
        .set_len(RULE_IMPORT_MAX_BYTES + 1)
        .expect("size import");
    let adapter = adapter_with(
        Arc::new(SqliteStore::in_memory().expect("store")),
        Arc::new(StaticOpenDialog { path: import }),
    );

    let error = adapter.import().await.expect_err("oversized import");
    assert_eq!(error.view_model.code, "IMPORT_TOO_LARGE");
}

#[tokio::test]
async fn malformed_persisted_rule_maps_to_persistence_corrupt() {
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    let workspace = ProxyWorkspace::default();
    let mut value = encode_workspace_record(&workspace).expect("workspace value");
    value["rules"] = serde_json::json!([{"not": "a rule"}]);
    store
        .insert_workspace(&WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value,
            updated_at: Utc::now(),
        })
        .expect("seed malformed workspace");
    let adapter = RuleRepositoryAdapter::new(
        store,
        Arc::new(NoDialog),
        Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
        &[],
    );

    let error = adapter.list().await.expect_err("corrupt rule");
    assert_eq!(error.view_model.code, "PERSISTENCE_CORRUPT");
    assert_ne!(error.view_model.code, "CERTIFICATE_INVALID");
}
