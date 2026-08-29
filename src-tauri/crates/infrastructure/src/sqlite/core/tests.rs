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
