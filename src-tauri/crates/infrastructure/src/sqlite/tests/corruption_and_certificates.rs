use super::*;

#[test]
fn schema_19_is_cleared_and_recreated_as_schema100() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let connection = Connection::open(&path).expect("open legacy database");
    connection
        .execute_batch(
            "CREATE TABLE application_schema (
                singleton_id INTEGER PRIMARY KEY,
                version INTEGER NOT NULL
             );
             INSERT INTO application_schema(singleton_id, version) VALUES (1, 19);
             CREATE TABLE legacy_listener_policy (value TEXT NOT NULL);
             INSERT INTO legacy_listener_policy(value) VALUES ('obsolete');",
        )
        .expect("seed schema 19");
    drop(connection);
    assert_eq!(CURRENT_APPLICATION_SCHEMA_VERSION, 100);
    drop(SqliteStore::open(&path).expect("schema 19 must recreate current storage"));
    let connection = Connection::open(&path).expect("reopen recreated database");
    let version = connection
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("Schema100 marker");
    let legacy_exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'legacy_listener_policy')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("legacy table probe");
    assert_eq!(version, 100);
    assert!(!legacy_exists, "pre-1.0 table must be deleted");
}

#[test]
fn committed_version_99_data_is_cleared_and_recreated_as_schema100() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let seed = Connection::open(&path).expect("open version 99 database");
    seed.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA wal_autocheckpoint = 0;
         CREATE TABLE application_schema(
             singleton_id INTEGER PRIMARY KEY,
             version INTEGER NOT NULL
         );
         INSERT INTO application_schema(singleton_id, version) VALUES (1, 99);
         CREATE TABLE wal_sentinel(value TEXT NOT NULL);
         INSERT INTO wal_sentinel(value) VALUES ('committed in WAL');",
    )
    .expect("commit version 99 WAL data");
    drop(seed);

    drop(SqliteStore::open(&path).expect("version 99 must recreate current storage"));
    let connection = Connection::open(&path).expect("reopen recreated database");
    let version = connection
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("Schema100 marker");
    let sentinel_exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'wal_sentinel')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("legacy WAL sentinel probe");
    assert_eq!(version, 100);
    assert!(!sentinel_exists);
}

#[test]
fn view_only_database_is_rejected_without_being_initialized() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let connection = Connection::open(&path).expect("open view-only database");
    connection
        .execute_batch("CREATE VIEW legacy_view AS SELECT 'must remain' AS value;")
        .expect("create view");
    drop(connection);
    let before = std::fs::read(&path).expect("read original database");

    assert!(matches!(
        SqliteStore::open(&path),
        Err(InfrastructureError::DatabaseSchemaInvalid { .. })
    ));
    assert_eq!(
        std::fs::read(&path).expect("read rejected database"),
        before
    );
    let connection = Connection::open(&path).expect("reopen view-only database");
    let value = connection
        .query_row("SELECT value FROM legacy_view", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("view remains readable");
    assert_eq!(value, "must remain");
}

#[test]
fn malformed_schema_markers_are_rejected_without_clearing_user_data() {
    let cases = [
        "CREATE TABLE application_schema(singleton_id INTEGER, version INTEGER)",
        "CREATE TABLE application_schema(singleton_id INTEGER, version INTEGER);
         INSERT INTO application_schema VALUES (2, 99)",
        "CREATE TABLE application_schema(singleton_id INTEGER, version INTEGER);
         INSERT INTO application_schema VALUES (1, 99);
         INSERT INTO application_schema VALUES (2, 98)",
        "CREATE TABLE application_schema(singleton_id INTEGER, version TEXT);
         INSERT INTO application_schema VALUES (1, 'broken')",
    ];
    for (index, marker_sql) in cases.into_iter().enumerate() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join(format!("invalid-{index}.sqlite"));
        let connection = Connection::open(&path).expect("open invalid database");
        connection.execute_batch(marker_sql).expect("seed marker");
        connection
            .execute_batch(
                "CREATE TABLE user_sentinel(value TEXT NOT NULL);
                 INSERT INTO user_sentinel(value) VALUES ('must remain');",
            )
            .expect("seed user data");
        drop(connection);

        assert!(SqliteStore::open(&path).is_err(), "case {index} must fail");
        let connection = Connection::open(&path).expect("reopen invalid database");
        let value = connection
            .query_row("SELECT value FROM user_sentinel", [], |row| {
                row.get::<_, String>(0)
            })
            .expect("user data remains");
        assert_eq!(value, "must remain");
    }
}

#[test]
fn certificate_batch_write_rolls_back_on_failure() {
    let store = SqliteStore::in_memory().expect("store");
    store
        .connection
        .lock()
        .execute_batch(
            "CREATE TRIGGER reject_leaf
             BEFORE INSERT ON certificate_material
             WHEN NEW.kind = 'proxy_leaf'
             BEGIN
                SELECT RAISE(ABORT, 'reject leaf');
             END;",
        )
        .expect("trigger");
    let now = Utc::now();
    let records = [
        CertificateMaterialRecord {
            kind: "local_root_ca".into(),
            protected_blob: vec![1],
            metadata: json!({"revision": 1}),
            updated_at: now,
        },
        CertificateMaterialRecord {
            kind: "proxy_leaf".into(),
            protected_blob: vec![2],
            metadata: json!({"revision": 1}),
            updated_at: now,
        },
    ];

    assert!(
        store
            .compare_and_swap_certificate_materials(0, &records)
            .is_err()
    );
    assert!(
        store
            .load_certificate_material("local_root_ca")
            .expect("load")
            .is_none()
    );
}

#[test]
fn certificate_aggregate_has_cross_connection_cas_and_atomic_snapshot() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    SqliteStore::open(&path).expect("initialize store");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let stores = [
        SqliteStore::open(&path).expect("first writer store"),
        SqliteStore::open(&path).expect("second writer store"),
    ];
    let writers = stores
        .into_iter()
        .zip([[1_u8, 2_u8], [3_u8, 4_u8]])
        .map(|(store, payloads)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let now = Utc::now();
                let records = [
                    CertificateMaterialRecord {
                        kind: "local_root_ca".into(),
                        protected_blob: vec![payloads[0]],
                        metadata: json!({"revision": 1}),
                        updated_at: now,
                    },
                    CertificateMaterialRecord {
                        kind: "proxy_leaf".into(),
                        protected_blob: vec![payloads[1]],
                        metadata: json!({"revision": 1}),
                        updated_at: now,
                    },
                ];
                barrier.wait();
                store.compare_and_swap_certificate_materials(0, &records)
            })
        })
        .collect::<Vec<_>>();
    let results = writers
        .into_iter()
        .map(|writer| writer.join().expect("writer thread"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(InfrastructureError::RevisionConflict)))
            .count(),
        1
    );

    let snapshot = SqliteStore::open(&path)
        .expect("reader store")
        .load_certificate_materials_snapshot(&["local_root_ca", "proxy_leaf"])
        .expect("snapshot");
    assert_eq!(snapshot.revision, 1);
    let protected_blobs = snapshot
        .records
        .iter()
        .map(|record| record.protected_blob.as_slice())
        .collect::<Vec<_>>();
    assert!(
        protected_blobs == vec![&[1][..], &[2][..]] || protected_blobs == vec![&[3][..], &[4][..]]
    );
    assert!(snapshot.records.iter().all(|record| {
        record.metadata.get("revision").and_then(Value::as_u64) == Some(snapshot.revision)
    }));
}

#[test]
fn certificate_reader_never_observes_a_mixed_writer_generation() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn records(revision: u64) -> [CertificateMaterialRecord; 2] {
        let payload = u8::try_from(revision).expect("small test revision");
        let now = Utc::now();
        [
            CertificateMaterialRecord {
                kind: "local_root_ca".into(),
                protected_blob: vec![payload],
                metadata: json!({"revision": revision}),
                updated_at: now,
            },
            CertificateMaterialRecord {
                kind: "proxy_leaf".into(),
                protected_blob: vec![payload],
                metadata: json!({"revision": revision}),
                updated_at: now,
            },
        ]
    }

    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let writer = SqliteStore::open(&path).expect("writer store");
    writer
        .compare_and_swap_certificate_materials(0, &records(1))
        .expect("seed aggregate");
    let reader = SqliteStore::open(&path).expect("reader store");
    let finished = Arc::new(AtomicBool::new(false));
    let reader_finished = finished.clone();

    let reader_task = std::thread::spawn(move || {
        let mut snapshots = 0;
        while !reader_finished.load(Ordering::Acquire) {
            let snapshot = reader
                .load_certificate_materials_snapshot(&["local_root_ca", "proxy_leaf"])
                .expect("atomic snapshot");
            assert_eq!(snapshot.records.len(), 2);
            assert_eq!(
                snapshot.records[0].protected_blob,
                snapshot.records[1].protected_blob
            );
            assert!(snapshot.records.iter().all(|record| {
                record.metadata.get("revision").and_then(Value::as_u64) == Some(snapshot.revision)
            }));
            snapshots += 1;
        }
        snapshots
    });

    for expected_revision in 1..25 {
        writer
            .compare_and_swap_certificate_materials(
                expected_revision,
                &records(expected_revision + 1),
            )
            .expect("advance aggregate");
    }
    finished.store(true, Ordering::Release);
    assert!(reader_task.join().expect("reader thread") > 0);
}
