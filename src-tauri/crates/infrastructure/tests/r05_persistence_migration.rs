use chrono::Utc;
use intercept_proxy_domain::{ProxyWorkspace, WorkspaceId};
use intercept_proxy_infrastructure::{InfrastructureErrorCode, SqliteStore};
use rusqlite::{Connection, params};
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn schema_ten_migrates_persisted_v2_v3_and_v4_workspaces_to_v5() {
    let database = database_initialized_at_version_nine();
    let connection = Connection::open(database.path()).expect("open v9 database");
    let fixtures = [
        legacy_workspace_value(2, 1),
        legacy_workspace_value(3, 4),
        legacy_workspace_value(4, 64),
    ];
    for value in &fixtures {
        insert_workspace(&connection, value);
    }
    drop(connection);

    drop(SqliteStore::open(database.path()).expect("migrate v9 database"));

    let connection = Connection::open(database.path()).expect("inspect migrated database");
    let versions = schema_versions(&connection);
    assert_eq!(versions, vec![9, 10]);
    let mut statement = connection
        .prepare("SELECT json FROM workspaces ORDER BY id")
        .expect("workspace query");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query workspaces")
        .collect::<Result<Vec<_>, _>>()
        .expect("workspace rows");
    assert_eq!(rows.len(), 3);
    for row in rows {
        let value: Value = serde_json::from_str(&row).expect("migrated workspace JSON");
        assert_eq!(value["_persistence_version"], 5);
        assert!(value.get("metadata_extractors").is_none());
    }
}

#[test]
fn invalid_workspace_rolls_back_all_rows_and_does_not_record_schema_ten() {
    let database = database_initialized_at_version_nine();
    let connection = Connection::open(database.path()).expect("open v9 database");
    let valid = legacy_workspace_value(4, 4);
    let valid_id = valid["id"].as_str().expect("workspace id").to_owned();
    insert_workspace(&connection, &valid);
    let mut invalid = legacy_workspace_value(4, 1);
    invalid["unknown_workspace_field"] = json!("must fail closed");
    insert_workspace(&connection, &invalid);
    drop(connection);

    let error = SqliteStore::open(database.path()).expect_err("bad row must abort migration");
    assert_eq!(
        error.code(),
        InfrastructureErrorCode::DatabaseMigrationFailed
    );

    let connection = Connection::open(database.path()).expect("inspect rolled-back database");
    assert_eq!(schema_versions(&connection), vec![9]);
    let valid_after: String = connection
        .query_row(
            "SELECT json FROM workspaces WHERE id = ?1",
            [valid_id],
            |row| row.get(0),
        )
        .expect("valid row remains");
    let valid_after: Value = serde_json::from_str(&valid_after).expect("valid legacy JSON");
    assert_eq!(valid_after["_persistence_version"], 4);
    assert_eq!(
        valid_after["metadata_extractors"].as_array().unwrap().len(),
        4
    );
}

struct TestDatabase {
    _directory: TempDir,
    path: std::path::PathBuf,
}

impl TestDatabase {
    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

fn database_initialized_at_version_nine() -> TestDatabase {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("r05.sqlite3");
    drop(SqliteStore::open(&path).expect("initialize current schema"));
    let connection = Connection::open(&path).expect("open initialized database");
    connection
        .execute("DELETE FROM schema_migrations WHERE version = 10", [])
        .expect("remove current ledger entry");
    connection
        .execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (9, ?1)",
            [Utc::now().to_rfc3339()],
        )
        .expect("seed version nine ledger entry");
    drop(connection);
    TestDatabase {
        _directory: directory,
        path,
    }
}

fn insert_workspace(connection: &Connection, value: &Value) {
    let id = value["id"].as_str().expect("workspace id");
    let revision = value["revision"].as_u64().expect("workspace revision");
    connection
        .execute(
            "INSERT INTO workspaces(id, revision, json, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                id,
                i64::try_from(revision).expect("revision fits SQLite"),
                value.to_string(),
                Utc::now().to_rfc3339()
            ],
        )
        .expect("insert legacy workspace");
}

fn schema_versions(connection: &Connection) -> Vec<i64> {
    let mut statement = connection
        .prepare("SELECT version FROM schema_migrations ORDER BY version")
        .expect("schema migration query");
    statement
        .query_map([], |row| row.get(0))
        .expect("schema migration rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("schema versions")
}

fn legacy_workspace_value(version: u16, extractor_count: usize) -> Value {
    let mut workspace = ProxyWorkspace {
        id: WorkspaceId::new(),
        name: format!("persisted v{version}"),
        ..ProxyWorkspace::default()
    };
    workspace.listeners[0].name = format!("v{version} listener");
    let mut value = serde_json::to_value(workspace).expect("workspace fixture");
    let object = value.as_object_mut().expect("workspace object");
    object.insert(
        "metadata_extractors".into(),
        Value::Array(legacy_extractors(extractor_count)),
    );
    match version {
        2 => {
            object.remove("socket_rules");
            object.remove("socket_rule_created_order_high_water");
            for listener in object["listeners"].as_array_mut().unwrap() {
                flatten_v2_http_listener(listener);
            }
        }
        3 => {
            object.insert("_persistence_version".into(), json!(3));
            object.remove("socket_rules");
            object.remove("socket_rule_created_order_high_water");
        }
        4 => {
            object.insert("_persistence_version".into(), json!(4));
        }
        _ => panic!("unsupported fixture version {version}"),
    }
    value
}

fn flatten_v2_http_listener(listener: &mut Value) {
    let object = listener.as_object_mut().expect("listener object");
    let data_plane = object.remove("data_plane").expect("listener data plane");
    assert_eq!(data_plane["kind"], "http");
    object.extend(data_plane["settings"].as_object().unwrap().clone());
}

fn legacy_extractors(count: usize) -> Vec<Value> {
    (0..count)
        .map(|index| {
            json!({
                "id": Uuid::new_v4(),
                "name": format!("extractor-{index}"),
                "listener_ids": [],
                "source": extractor_source(index)
            })
        })
        .collect()
}

fn extractor_source(index: usize) -> Value {
    let source: Map<String, Value> = match index % 4 {
        0 => Map::from_iter([
            ("kind".into(), json!("header")),
            ("name".into(), json!("x-request-id")),
        ]),
        1 => Map::from_iter([
            ("kind".into(), json!("json_path")),
            ("path".into(), json!("$.transaction.id")),
        ]),
        2 => Map::from_iter([("kind".into(), json!("body_text"))]),
        _ => Map::from_iter([
            ("kind".into(), json!("fixed_value")),
            ("value".into(), json!("fixed")),
        ]),
    };
    Value::Object(source)
}
