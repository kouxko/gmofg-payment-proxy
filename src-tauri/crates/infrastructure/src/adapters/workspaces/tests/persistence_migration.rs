#[tokio::test]
async fn current_workspace_persistence_round_trips() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory store"));
    let workspace = ProxyWorkspace::default();
    store
        .insert_workspace(&WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value: encode_workspace_record(&workspace).expect("current workspace JSON"),
            updated_at: chrono::Utc::now(),
        })
        .expect("seed current workspace JSON");

    let repository = WorkspaceRepositoryAdapter::new(store);
    assert_eq!(repository.get(workspace.id).await.unwrap(), workspace);
}

#[test]
fn version_six_upgrade_persists_removal_of_unbound_standard_rules() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("workspace-migration.sqlite3");
    let listener = ProxyListener::default();
    let bound_channel = ChannelId::new(listener.id.to_string()).expect("channel");
    let rule = |name: &str, channel| Rule {
        id: RuleId::new(),
        revision: DomainRevision::INITIAL,
        name: name.into(),
        description: String::new(),
        enabled: true,
        priority: 10,
        created_order: 1,
        channel,
        stage: MessageStage::Request,
        conditions: Vec::new(),
        actions: vec![RuleAction::Delay { milliseconds: 10 }],
        one_shot: false,
        hit_count: 0,
        last_hit_at: None,
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener],
        rules: vec![
            rule("unbound", None),
            rule("bound", Some(bound_channel)),
        ],
        ..ProxyWorkspace::default()
    };
    let store = SqliteStore::open(&path).expect("create store");
    let mut value = encode_workspace_record(&workspace).expect("workspace JSON");
    value["_persistence_version"] = serde_json::json!(6);
    store
        .insert_workspace(&WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value,
            updated_at: chrono::Utc::now(),
        })
        .expect("seed version six workspace");
    drop(store);

    let reopened = Arc::new(SqliteStore::open(&path).expect("upgrade store"));
    let record = reopened
        .load_workspace(workspace.id.as_uuid())
        .expect("load migrated record")
        .expect("workspace remains");
    assert_eq!(record.revision, workspace.revision.get() + 1);
    assert_eq!(record.value["_persistence_version"], serde_json::json!(7));
    let migrated = decode_workspace_record(record).expect("decode migrated workspace");
    assert_eq!(migrated.rules.len(), 1);
    assert_eq!(migrated.rules[0].name, "bound");
    assert_eq!(migrated.rules[0].channel.as_ref(), Some(&ChannelId::new(migrated.listeners[0].id.to_string()).expect("channel")));
}

#[test]
fn version_six_upgrade_rolls_back_every_workspace_when_a_later_record_is_invalid() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("workspace-migration-rollback.sqlite3");
    let make_workspace = |name: &str| {
        let listener = ProxyListener::default();
        let channel = ChannelId::new(listener.id.to_string()).expect("channel");
        let make_rule = |rule_name: &str, channel| Rule {
            id: RuleId::new(),
            revision: DomainRevision::INITIAL,
            name: rule_name.into(),
            description: String::new(),
            enabled: true,
            priority: 10,
            created_order: 1,
            channel,
            stage: MessageStage::Request,
            conditions: Vec::new(),
            actions: vec![RuleAction::Delay { milliseconds: 10 }],
            one_shot: false,
            hit_count: 0,
            last_hit_at: None,
        };
        ProxyWorkspace {
            name: name.into(),
            listeners: vec![listener],
            rules: vec![
                make_rule("unbound", None),
                make_rule("bound", Some(channel)),
            ],
            ..ProxyWorkspace::default()
        }
    };
    let mut workspaces = vec![make_workspace("First"), make_workspace("Second")];
    workspaces.sort_by_key(|workspace| workspace.id.to_string());
    let store = SqliteStore::open(&path).expect("create store");
    let mut version_six_values = Vec::new();
    for (index, workspace) in workspaces.iter().enumerate() {
        let mut value = encode_workspace_record(workspace).expect("workspace JSON");
        value["_persistence_version"] = serde_json::json!(6);
        if index == 1 {
            value["rules"][1]["channel"] = serde_json::json!(17);
        }
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value: value.clone(),
                updated_at: chrono::Utc::now(),
            })
            .expect("seed version six workspace");
        version_six_values.push(value);
    }
    drop(store);

    assert!(SqliteStore::open(&path).is_err());
    let connection = rusqlite::Connection::open(&path).expect("inspect rolled back database");
    for (workspace, original) in workspaces.iter().zip(&version_six_values) {
        let (revision, json): (i64, String) = connection
            .query_row(
                "SELECT revision, json FROM workspaces WHERE id = ?1",
                [workspace.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("rolled back workspace");
        assert_eq!(revision, i64::try_from(workspace.revision.get()).unwrap());
        assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap(), *original);
    }

    let mut repaired = version_six_values[1].clone();
    repaired["rules"][1]["channel"] =
        serde_json::json!(workspaces[1].listeners[0].id.to_string());
    connection
        .execute(
            "UPDATE workspaces SET json = ?1 WHERE id = ?2",
            rusqlite::params![repaired.to_string(), workspaces[1].id.to_string()],
        )
        .expect("repair second workspace");
    drop(connection);

    let migrated = SqliteStore::open(&path).expect("migrate both workspaces");
    for workspace in &workspaces {
        let record = migrated
            .load_workspace(workspace.id.as_uuid())
            .expect("load migrated record")
            .expect("workspace remains");
        assert_eq!(record.revision, workspace.revision.get() + 1);
        assert_eq!(record.value["_persistence_version"], serde_json::json!(7));
        let decoded = decode_workspace_record(record).expect("decode migrated workspace");
        assert_eq!(decoded.rules.len(), 1);
        assert_eq!(
            decoded.rules[0].channel.as_ref(),
            Some(&ChannelId::new(decoded.listeners[0].id.to_string()).expect("channel")),
        );
    }
}

#[tokio::test]
async fn missing_or_old_workspace_persistence_version_is_rejected() {
    for version in [None, Some(5_u16)] {
        let store = Arc::new(SqliteStore::in_memory().expect("in-memory store"));
        let workspace = ProxyWorkspace::default();
        let mut value = serde_json::to_value(&workspace).unwrap();
        if let Some(version) = version {
            value["_persistence_version"] = serde_json::json!(version);
        }
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value,
                updated_at: chrono::Utc::now(),
            })
            .expect("seed unsupported workspace JSON");

        let repository = WorkspaceRepositoryAdapter::new(store);
        let error = repository.get(workspace.id).await.unwrap_err();
        assert_eq!(error.view_model.code, "PERSISTENCE_CORRUPT");
    }
}

#[tokio::test]
async fn current_workspace_rejects_unknown_fields() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory store"));
    let workspace = ProxyWorkspace::default();
    let mut value = encode_workspace_record(&workspace).unwrap();
    value["future_field"] = serde_json::json!("must not load");
    store
        .insert_workspace(&WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value,
            updated_at: chrono::Utc::now(),
        })
        .expect("seed invalid workspace JSON");

    let repository = WorkspaceRepositoryAdapter::new(store);
    let error = repository.get(workspace.id).await.unwrap_err();
    assert_eq!(error.view_model.code, "PERSISTENCE_CORRUPT");
    assert!(!error.view_model.message.contains("must not load"));
}
