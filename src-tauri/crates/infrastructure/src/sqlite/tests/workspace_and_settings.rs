use super::*;

#[test]
fn schema_keeps_http_payload_storage_absent_and_socket_capture_explicit() {
    let store = SqliteStore::in_memory().expect("store");
    let tables = store.table_names().expect("tables");
    assert_eq!(
        tables,
        vec![
            "android_runtime_owner",
            "certificate_material",
            "certificate_state",
            "protected_secrets",
            "protocol_package_files",
            "protocol_packages",
            "rule_state",
            "rules",
            "schema_migrations",
            "settings",
            "socket_captures",
            "sqlite_sequence",
            "workspace_state",
            "workspaces"
        ]
    );
    assert!(!tables.iter().any(|name| name.contains("http_capture")
        || name.contains("http_session")
        || name.contains("breakpoint")));
    let secret_columns = store
        .connection
        .lock()
        .prepare("PRAGMA table_info(protected_secrets)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .expect("protected secret columns");
    assert!(secret_columns.contains(&"protected_blob".to_owned()));
    assert!(!secret_columns.iter().any(|column| {
        column.contains("password") || column.contains("username") || column == "plaintext"
    }));
}

#[test]
fn workspaces_persist_selection_and_use_optimistic_revisions() {
    let store = SqliteStore::in_memory().expect("store");
    let first_id = Uuid::new_v4();
    let first = WorkspaceRecord {
        id: first_id,
        revision: 1,
        value: json!({"id": first_id, "name": "first", "revision": 1}),
        updated_at: Utc::now(),
    };
    store.insert_workspace(&first).expect("insert first");
    let snapshot = store.load_workspaces().expect("snapshot");
    assert_eq!(snapshot.selected_id, Some(first_id));
    assert_eq!(snapshot.records, vec![first.clone()]);

    let mut updated = first.clone();
    updated.revision = 2;
    updated.value["revision"] = json!(2);
    store
        .compare_and_swap_workspace(1, &updated)
        .expect("advance revision");
    assert!(matches!(
        store.compare_and_swap_workspace(1, &updated),
        Err(InfrastructureError::RevisionConflict)
    ));

    let second_id = Uuid::new_v4();
    store
        .insert_workspace(&WorkspaceRecord {
            id: second_id,
            revision: 1,
            value: json!({"id": second_id, "name": "second", "revision": 1}),
            updated_at: Utc::now(),
        })
        .expect("insert second");
    store.select_workspace(second_id).expect("select second");
    assert!(matches!(
        store.compare_and_swap_selected_workspace(first_id, 2, &updated),
        Err(InfrastructureError::RevisionConflict)
    ));
    let second_updated = WorkspaceRecord {
        id: second_id,
        revision: 2,
        value: json!({"id": second_id, "name": "second updated", "revision": 2}),
        updated_at: Utc::now(),
    };
    store
        .compare_and_swap_selected_workspace(second_id, 1, &second_updated)
        .expect("update selected workspace");
    assert_eq!(
        store
            .load_workspaces()
            .expect("selected snapshot")
            .selected_id,
        Some(second_id)
    );
    store.delete_workspace(second_id, 2).expect("delete second");
    assert_eq!(
        store
            .load_workspaces()
            .expect("fallback selection")
            .selected_id,
        Some(first_id)
    );
}

#[test]
fn full_configuration_replace_rolls_back_all_tables_on_failure() {
    let store = SqliteStore::in_memory().expect("store");
    let original_id = Uuid::new_v4();
    let original = WorkspaceRecord {
        id: original_id,
        revision: 1,
        value: json!({"id": original_id, "name": "original", "revision": 1}),
        updated_at: Utc::now(),
    };
    store.insert_workspace(&original).expect("seed workspace");
    store
        .save_settings(0, &json!({"name": "original settings"}))
        .expect("seed settings");

    let duplicate_id = Uuid::new_v4();
    let duplicate = WorkspaceRecord {
        id: duplicate_id,
        revision: 1,
        value: json!({"id": duplicate_id, "name": "replacement", "revision": 1}),
        updated_at: Utc::now(),
    };
    let error = store
        .replace_application_configuration(
            duplicate_id,
            &[duplicate.clone(), duplicate],
            &json!({"name": "replacement settings"}),
        )
        .expect_err("duplicate insert must fail inside transaction");
    assert!(matches!(error, InfrastructureError::Database { .. }));

    let snapshot = store.load_workspaces().expect("workspace snapshot");
    assert_eq!(snapshot.selected_id, Some(original_id));
    assert_eq!(snapshot.records, vec![original]);
    assert_eq!(
        store
            .load_settings()
            .expect("settings")
            .expect("stored")
            .value,
        json!({"name": "original settings"})
    );
}

#[test]
fn application_data_reset_atomically_removes_persisted_user_data() {
    let store = SqliteStore::in_memory().expect("store");
    let old_id = Uuid::new_v4();
    store
        .insert_workspace(&WorkspaceRecord {
            id: old_id,
            revision: 1,
            value: json!({"id": old_id, "name": "old", "revision": 1}),
            updated_at: Utc::now(),
        })
        .expect("seed workspace");
    store
        .save_settings(0, &json!({"name": "old settings"}))
        .expect("seed settings");
    store
        .save_protected_secret(&ProtectedSecretRecord {
            provider: "test".into(),
            key: "listener-p12".into(),
            protected_blob: vec![1, 2, 3],
            updated_at: Utc::now(),
        })
        .expect("seed protected secret");
    {
        let connection = store.connection.lock();
        connection
            .execute(
                "INSERT INTO rules(id, revision, enabled, json, updated_at) VALUES (?1, 1, 1, ?2, ?3)",
                params![Uuid::new_v4().to_string(), "{}", Utc::now().to_rfc3339()],
            )
            .expect("seed rule");
        connection
            .execute(
                "INSERT INTO certificate_material(kind, protected_blob, metadata_json, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params!["listener_identity", vec![4_u8, 5], "{}", Utc::now().to_rfc3339()],
            )
            .expect("seed certificate");
        connection
            .execute(
                "INSERT INTO socket_captures(
                    capture_id, runtime_epoch, workspace_id, listener_id, session_id,
                    connection_id, occurred_at, completed_at, kind, direction,
                    package_id, package_version, logical_bytes, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 'relay_frame',
                           'upstream_receive', 'example', '1.0.0', 1, '{}')",
                params![
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    old_id.to_string(),
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    Utc::now().to_rfc3339(),
                ],
            )
            .expect("seed socket capture");
    }

    let clean_id = Uuid::new_v4();
    let clean = WorkspaceRecord {
        id: clean_id,
        revision: 1,
        value: json!({"id": clean_id, "name": "Default Workspace", "revision": 1}),
        updated_at: Utc::now(),
    };
    store
        .reset_application_data(
            clean_id,
            std::slice::from_ref(&clean),
            &json!({"default": true}),
        )
        .expect("atomic reset");

    let snapshot = store.load_workspaces().expect("workspace snapshot");
    assert_eq!(snapshot.selected_id, Some(clean_id));
    assert_eq!(snapshot.records, vec![clean]);
    assert_eq!(
        store
            .load_settings()
            .expect("settings")
            .expect("stored")
            .value,
        json!({"default": true})
    );
    assert!(
        store
            .load_protected_secret("test", "listener-p12")
            .expect("secret lookup")
            .is_none()
    );
    let connection = store.connection.lock();
    for table in ["rules", "certificate_material", "socket_captures"] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("row count");
        assert_eq!(count, 0, "{table} must be empty");
    }
}

/// ENGINE-008, SECURITY-004: stale settings writes fail atomically.
#[test]
fn settings_use_optimistic_revision() {
    let store = SqliteStore::in_memory().expect("store");
    let first = store
        .save_settings(0, &json!({"port": 16627}))
        .expect("save");
    assert_eq!(first.revision, 1);
    assert!(matches!(
        store.save_settings(0, &json!({"port": 16127})),
        Err(InfrastructureError::RevisionConflict)
    ));
    assert_eq!(
        store.load_settings().expect("load").expect("value").value,
        json!({"port": 16627})
    );
}

#[test]
fn corrupt_settings_revision_json_and_time_use_persistence_error_classification() {
    for (column, value) in [
        ("revision", "-1"),
        ("json", "{not-json"),
        ("updated_at", "not-a-time"),
    ] {
        let store = SqliteStore::in_memory().expect("store");
        store
            .save_settings(0, &json!({"channel": "alpha"}))
            .expect("seed settings");
        let sql = format!("UPDATE settings SET {column} = ?1 WHERE singleton_id = 1");
        store
            .connection
            .lock()
            .execute(&sql, [value])
            .expect("corrupt settings");

        let error = store
            .load_settings()
            .expect_err("corrupt settings must fail");
        assert_eq!(
            error.code(),
            crate::InfrastructureErrorCode::PersistenceCorrupt,
            "wrong classification for {column}"
        );
        assert!(!matches!(
            error,
            InfrastructureError::CertificateInvalid { .. }
        ));
    }
}
