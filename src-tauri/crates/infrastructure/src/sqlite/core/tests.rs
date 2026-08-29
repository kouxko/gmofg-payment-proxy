use super::*;

fn seed_pre_baseline_database(connection: &Connection) {
    connection
        .execute_batch(
            "CREATE TABLE application_schema(
                singleton_id INTEGER PRIMARY KEY,
                version INTEGER NOT NULL
             );
             INSERT INTO application_schema(singleton_id, version) VALUES (1, 99);
             CREATE TABLE pre_baseline_sentinel(value TEXT NOT NULL);
             INSERT INTO pre_baseline_sentinel(value) VALUES ('must remain on rollback');
             CREATE TABLE a_parent(id INTEGER PRIMARY KEY);
             CREATE TABLE z_child(
                 parent_id INTEGER NOT NULL REFERENCES a_parent(id) ON DELETE RESTRICT
             );
             INSERT INTO a_parent(id) VALUES (1);
             INSERT INTO z_child(parent_id) VALUES (1);",
        )
        .expect("seed version 99 database");
}

#[test]
fn delayed_pre_baseline_reset_rechecks_version_after_writer_lock() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let mut first = Connection::open(&path).expect("first connection");
    seed_pre_baseline_database(&first);
    let mut delayed = Connection::open(&path).expect("delayed connection");
    assert_eq!(
        classify_existing_schema(&delayed).expect("initial classification"),
        ExistingSchema::PreCompatibilityBaseline
    );

    reset_pre_compatibility_schema(&mut first).expect("first reset");
    first
        .execute_batch(
            "CREATE TABLE version_100_writer(value TEXT NOT NULL);
             INSERT INTO version_100_writer(value) VALUES ('formal data');",
        )
        .expect("write formal version 100 data");

    reset_pre_compatibility_schema(&mut delayed).expect("delayed reset becomes no-op");
    let value = delayed
        .query_row("SELECT value FROM version_100_writer", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("formal data remains");
    let foreign_keys = delayed
        .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))
        .expect("foreign key state");
    assert_eq!(value, "formal data");
    assert!(foreign_keys);
}

#[test]
fn failed_pre_baseline_rebuild_rolls_back_every_drop() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let mut connection = Connection::open(&path).expect("connection");
    seed_pre_baseline_database(&connection);

    let result = reset_pre_compatibility_schema_with(&mut connection, |_| {
        Err(InfrastructureError::PersistenceCorrupt {
            entity: "schema_reset_test",
            message: "injected initialization failure".to_owned(),
        })
    });
    assert!(result.is_err());
    let marker = connection
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("pre-baseline marker remains");
    let sentinel = connection
        .query_row("SELECT value FROM pre_baseline_sentinel", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("pre-baseline data remains");
    let foreign_keys = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))
        .expect("foreign key state");
    assert_eq!(marker, 99);
    assert_eq!(sentinel, "must remain on rollback");
    assert!(foreign_keys);
}

#[test]
fn delayed_empty_database_initialization_rechecks_and_resets_pre_baseline_schema() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let delayed = Connection::open(&path).expect("delayed empty connection");
    delayed
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable production foreign key behavior");
    assert!(database_is_empty(&delayed).expect("initial empty classification"));

    let writer = Connection::open(&path).expect("pre-baseline writer");
    seed_pre_baseline_database(&writer);
    drop(writer);
    let store = SqliteStore {
        connection: Mutex::new(delayed),
        blocking_gate: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    };
    store
        .create_schema()
        .expect("recheck and reset version 99 schema");

    let connection = store.connection.lock();
    assert_eq!(
        classify_existing_schema(&connection).expect("current schema"),
        ExistingSchema::Current
    );
    let sentinel_exists = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_schema
                WHERE type = 'table' AND name = 'pre_baseline_sentinel'
            )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("sentinel probe");
    assert!(!sentinel_exists);
    let foreign_keys = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))
        .expect("foreign key state");
    assert!(foreign_keys);
}

#[test]
fn recreate_current_replaces_pre_baseline_old_layout_and_current_schema() {
    for seed in [
        "CREATE TABLE application_schema(singleton_id INTEGER PRIMARY KEY, version INTEGER NOT NULL);\
         INSERT INTO application_schema(singleton_id, version) VALUES (1, 99);\
         CREATE TABLE phase2_sentinel(value TEXT NOT NULL);\
         INSERT INTO phase2_sentinel(value) VALUES ('pre-baseline');",
        "CREATE TABLE application_schema(singleton_id INTEGER PRIMARY KEY, version INTEGER NOT NULL);\
         INSERT INTO application_schema(singleton_id, version) VALUES (1, 100);\
         CREATE TABLE phase2_sentinel(value TEXT NOT NULL);\
         INSERT INTO phase2_sentinel(value) VALUES ('old-layout-100');",
    ] {
        let mut connection = Connection::open_in_memory().expect("connection");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        connection.execute_batch(seed).expect("seed old database");

        recreate_current_schema(&mut connection).expect("recreate current Schema100");

        assert_recreated_current_schema(&connection);
    }

    let mut connection = Connection::open_in_memory().expect("current connection");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("enable foreign keys");
    let transaction = connection.transaction().expect("schema transaction");
    initialize_current_schema(&transaction).expect("current schema");
    transaction.commit().expect("commit current schema");
    connection
        .execute_batch(
            "CREATE VIEW phase2_view AS SELECT version FROM application_schema;\
             CREATE TRIGGER phase2_trigger AFTER INSERT ON settings BEGIN SELECT 1; END;",
        )
        .expect("seed current layout objects");

    recreate_current_schema(&mut connection).expect("recreate current Schema100");

    assert_recreated_current_schema(&connection);
}

#[test]
fn failed_recreate_rolls_back_all_objects_and_restores_foreign_keys() {
    let mut connection = Connection::open_in_memory().expect("connection");
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;\
             CREATE TABLE phase2_sentinel(value TEXT NOT NULL);\
             INSERT INTO phase2_sentinel(value) VALUES ('must remain');",
        )
        .expect("seed database");

    let error = recreate_current_schema_with(&mut connection, |_| {
        Err(InfrastructureError::PersistenceCorrupt {
            entity: "phase2_recreate_test",
            message: "injected initialization failure".to_owned(),
        })
    })
    .expect_err("recreate failure must propagate");

    assert!(matches!(
        error,
        InfrastructureError::PersistenceCorrupt {
            entity: "phase2_recreate_test",
            ..
        }
    ));
    assert_eq!(
        connection
            .query_row("SELECT value FROM phase2_sentinel", [], |row| row
                .get::<_, String>(0))
            .expect("rolled-back sentinel"),
        "must remain"
    );
    assert!(
        connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))
            .expect("foreign key state")
    );
}

#[test]
fn recreate_current_replaces_data_committed_in_wal() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let writer = Connection::open(&path).expect("writer");
    writer
        .execute_batch(
            "PRAGMA journal_mode = WAL;\
             CREATE TABLE phase2_wal_sentinel(value TEXT NOT NULL);\
             INSERT INTO phase2_wal_sentinel(value) VALUES ('committed in WAL');",
        )
        .expect("commit WAL-backed data");
    let wal_path = path.with_file_name("state.sqlite-wal");
    assert!(wal_path.is_file(), "committed WAL exists before recreate");

    let store = SqliteStore::open_with_startup_policy(&path, SqliteStartupPolicy::RecreateCurrent)
        .expect("recreate committed WAL database");
    let connection = store.connection.lock();
    assert_recreated_current_schema(&connection);
    drop(connection);
    drop(store);
    drop(writer);
}

fn assert_recreated_current_schema(connection: &Connection) {
    assert_eq!(
        connection
            .query_row(
                "SELECT version FROM application_schema WHERE singleton_id = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("Schema100 marker"),
        100
    );
    let leftover = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name LIKE 'phase2_%')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("leftover object probe");
    assert!(!leftover, "old tables, views, and triggers must be removed");
    assert!(
        connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, bool>(0))
            .expect("foreign key state")
    );
}
