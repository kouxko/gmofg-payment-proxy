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
