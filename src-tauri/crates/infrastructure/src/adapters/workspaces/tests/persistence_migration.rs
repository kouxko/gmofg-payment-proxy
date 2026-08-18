#[tokio::test]
async fn persisted_v2_workspace_reads_without_writing_and_next_save_persists_v5() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory store"));
    let workspace = ProxyWorkspace::default();
    store
        .insert_workspace(&WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value: persisted_v2_value(&workspace),
            updated_at: chrono::Utc::now(),
        })
        .expect("seed v2 workspace JSON");
    let repository = WorkspaceRepositoryAdapter::new(Arc::clone(&store));

    let mut migrated = repository.get(workspace.id).await.expect("read v2 row");

    assert_eq!(migrated.id, workspace.id);
    assert_eq!(migrated.revision, workspace.revision);
    assert!(matches!(
        migrated.listeners[0].data_plane,
        ListenerDataPlane::Http(_)
    ));
    let stored_after_read = store.load_workspaces().expect("reload v2 row");
    assert_eq!(
        stored_after_read.records[0].revision,
        workspace.revision.get()
    );
    assert!(
        stored_after_read.records[0]
            .value
            .get("_persistence_version")
            .is_none()
    );
    assert!(
        stored_after_read.records[0].value["listeners"][0]
            .get("data_plane")
            .is_none()
    );

    migrated.name = "Saved after migration".into();
    let saved = repository.save(migrated).await.expect("save migrated row");
    let stored_after_save = store.load_workspaces().expect("reload v5 row");
    assert_eq!(stored_after_save.records[0].revision, saved.revision.get());
    assert_eq!(
        stored_after_save.records[0].value["_persistence_version"],
        5
    );
    assert!(stored_after_save.records[0].value["listeners"][0]["data_plane"].is_object());
}

#[tokio::test]
async fn persisted_v3_workspace_defaults_socket_rule_state_and_next_save_persists_v5() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory store"));
    let workspace = ProxyWorkspace::default();
    let mut value = serde_json::to_value(&workspace).unwrap();
    let object = value.as_object_mut().unwrap();
    object.insert("metadata_extractors".into(), serde_json::json!([]));
    object.remove("socket_rules");
    object.remove("socket_rule_created_order_high_water");
    object.insert("_persistence_version".into(), serde_json::json!(3));
    store
        .insert_workspace(&WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value,
            updated_at: chrono::Utc::now(),
        })
        .expect("seed v3 workspace JSON");
    let repository = WorkspaceRepositoryAdapter::new(Arc::clone(&store));

    let mut migrated = repository.get(workspace.id).await.expect("read v3 row");
    assert!(migrated.socket_rules.is_empty());
    assert_eq!(migrated.socket_rule_created_order_high_water, 0);
    migrated.name = "Saved after v3 migration".into();
    repository.save(migrated).await.expect("save migrated row");

    let stored = store.load_workspaces().expect("reload v5 row");
    assert_eq!(stored.records[0].value["_persistence_version"], 5);
    assert!(stored.records[0].value["socket_rules"].is_array());
    assert_eq!(
        stored.records[0].value["socket_rule_created_order_high_water"],
        0
    );
}

#[tokio::test]
async fn persisted_v4_workspace_rejects_each_missing_socket_rule_field() {
    for missing_field in ["socket_rules", "socket_rule_created_order_high_water"] {
        let store = Arc::new(SqliteStore::in_memory().expect("in-memory store"));
        let workspace = ProxyWorkspace::default();
        let mut value = encode_workspace_record(&workspace).expect("v4 workspace JSON");
        value["_persistence_version"] = serde_json::json!(4);
        value
            .as_object_mut()
            .expect("workspace object")
            .remove(missing_field);
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value,
                updated_at: chrono::Utc::now(),
            })
            .expect("seed damaged v4 workspace JSON");

        let repository = WorkspaceRepositoryAdapter::new(store);
        let error = repository
            .get(workspace.id)
            .await
            .expect_err("missing v4 field must fail closed");
        assert_eq!(error.view_model.code, "PERSISTENCE_CORRUPT");
    }
}

#[tokio::test]
async fn persisted_v2_workspace_rejects_unknown_and_secret_fields() {
    for field in ["future_field", "proxy_password"] {
        let store = Arc::new(SqliteStore::in_memory().expect("in-memory store"));
        let workspace = ProxyWorkspace::default();
        let mut legacy = persisted_v2_value(&workspace);
        legacy[field] = Value::String("must not load".into());
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value: legacy,
                updated_at: chrono::Utc::now(),
            })
            .expect("seed corrupt v2 workspace JSON");
        let repository = WorkspaceRepositoryAdapter::new(store);

        let error = repository
            .get(workspace.id)
            .await
            .expect_err("strict v2 shape must reject unknown fields");

        assert_eq!(error.view_model.code, "PERSISTENCE_CORRUPT");
        assert!(!error.view_model.message.contains("must not load"));
    }
}

fn persisted_v2_value(workspace: &ProxyWorkspace) -> Value {
    let listener = &workspace.listeners[0];
    let http = listener.http().expect("default listener is HTTP");
    serde_json::json!({
        "id": workspace.id,
        "name": workspace.name,
        "revision": workspace.revision,
        "listeners": [{
            "id": listener.id,
            "name": listener.name,
            "enabled": listener.enabled,
            "bind_address": listener.bind_address,
            "port": listener.port,
            "authentication": http.authentication,
            "allowed_client_cidrs": listener.allowed_client_cidrs,
            "mitm": http.mitm,
            "connect_timeout_ms": listener.connect_timeout_ms,
            "read_timeout_ms": listener.read_timeout_ms,
            "write_timeout_ms": listener.write_timeout_ms,
            "downstream_tls": http.downstream_tls,
            "request_body_codec": http.request_body_codec,
            "response_body_codec": http.response_body_codec,
            "fixed_server": http.fixed_server,
        }],
        "metadata_extractors": [],
        "response_assertions": workspace.response_assertions,
        "rules": workspace.rules,
        "fault_presets": workspace.fault_presets,
        "certificate_references": workspace.certificate_references,
        "android_network_profiles": workspace.android_network_profiles,
    })
}
