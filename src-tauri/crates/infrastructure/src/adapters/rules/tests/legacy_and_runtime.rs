use super::*;

#[tokio::test]
async fn legacy_global_rule_table_is_not_a_runtime_or_crud_source() {
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    seed_workspace(&store, Vec::new());
    let legacy = Rule::create(
        to_domain_draft(&request_delay_draft("legacy global row", false), 1).expect("legacy draft"),
    )
    .expect("legacy rule");
    let mut value = serde_json::to_value(&legacy).expect("legacy value");
    value.as_object_mut().expect("rule object").insert(
        PERSISTENCE_VERSION_FIELD.into(),
        Value::from(RULE_PERSISTENCE_VERSION),
    );
    store
        .insert_rule(&crate::RuleRecord {
            id: legacy.id.as_uuid(),
            revision: legacy.revision.get(),
            enabled: legacy.enabled,
            value,
            updated_at: Utc::now(),
        })
        .expect("seed legacy global row");

    let adapter = adapter_with(store, Arc::new(NoDialog));
    assert!(adapter.list().await.expect("workspace rules").is_empty());
    assert!(runtime_snapshot(&adapter).rules.is_empty());
}

#[tokio::test]
async fn legacy_rule_json_import_migrates_shift_jis_body() {
    let directory = tempfile::tempdir().expect("temp directory");
    let import = directory.path().join("legacy-rules.json");
    let id = uuid::Uuid::new_v4();
    let legacy = legacy_rule_json(
        id,
        &serde_json::json!({
            "MockResponse": {
                "status": 200,
                "headers": [],
                "shift_jis_body": [123, 125]
            }
        }),
    );
    std::fs::write(
        &import,
        serde_json::to_vec_pretty(&vec![legacy]).expect("legacy JSON"),
    )
    .expect("write legacy JSON");
    let store = Arc::new(SqliteStore::in_memory().expect("store"));
    seed_workspace(&store, Vec::new());
    let adapter = RuleRepositoryAdapter::new(
        store,
        Arc::new(StaticOpenDialog { path: import }),
        Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
        &[],
        &["shift_jis_body"],
    );

    let result = adapter.import().await.expect("import legacy JSON");
    assert!(result.success);
    let loaded = adapter.get(id).await.expect("imported legacy rule");
    assert!(matches!(
        loaded.draft.actions.as_slice(),
        [AppRuleAction::Terminal {
            action: AppRuleTerminalAction::MockResponse { body_bytes, .. }
        }] if body_bytes == b"{}"
    ));
}

#[test]
fn legacy_invalid_json_terminal_body_has_an_explicit_v0_compatibility_path() {
    let id = uuid::Uuid::new_v4();
    let rule = deserialize_persisted_rule(
        legacy_rule_json(
            id,
            &serde_json::json!({"InvalidJson": {"shift_jis_body": [123]}}),
        ),
        &["shift_jis_body"],
    )
    .expect("migrate legacy InvalidJson body");
    assert!(matches!(
        rule.actions.as_slice(),
        [RuleAction::Terminal(TerminalAction::InvalidJson { body_bytes })]
            if body_bytes == b"{"
    ));
}

#[tokio::test]
async fn runtime_commit_is_full_signature_cas_and_reset_preserves_enabled() {
    let adapter = adapter();
    let created = adapter
        .save(request_delay_draft("one-shot", true))
        .await
        .expect("create");
    let snapshot = runtime_snapshot(&adapter);
    let epoch = RuntimeEpoch::new();
    let terminal = TerminalIdentity {
        source_ip: "127.0.0.1".into(),
        certificate_sha256: String::new(),
    };
    let mut engine = RuleEngine::new(epoch, snapshot.rules.clone());
    engine.evaluate(
        &MatchContext {
            runtime_epoch: epoch,
            channel: ChannelId::new("alpha").unwrap(),
            stage: MessageStage::Request,
            terminal: &terminal,
            path_or_request_type: None,
            json_body: None,
        },
        Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
    );
    adapter
        .commit_runtime_snapshot(&snapshot, engine.rules())
        .expect("runtime commit");

    let fired = adapter
        .get_domain(created.summary.rule_id)
        .expect("fired rule");
    assert!(!fired.enabled);
    assert_eq!(fired.revision, Revision::new(2));
    assert_eq!(fired.hit_count, 1);
    assert!(fired.last_hit_at.is_some());

    adapter
        .reset_runtime_hit_metadata(snapshot.collection_id.expect("workspace id"))
        .expect("explicit reset");
    let reset = adapter
        .get_domain(created.summary.rule_id)
        .expect("reset rule");
    assert!(!reset.enabled);
    assert_eq!(reset.revision, Revision::new(2));
    assert_eq!(reset.hit_count, 0);
    assert_eq!(reset.last_hit_at, None);

    let stale = runtime_snapshot(&adapter);
    adapter
        .toggle_domain(created.summary.rule_id, 2, true)
        .expect("concurrent config update");
    let error = adapter
        .commit_runtime_snapshot(&stale, &stale.rules)
        .expect_err("stale runtime commit");
    assert_eq!(error.view_model.code, "REVISION_CONFLICT");
    let configured = adapter
        .get_domain(created.summary.rule_id)
        .expect("configured rule");
    assert!(configured.enabled);
    assert_eq!(configured.revision, Revision::new(3));
}
