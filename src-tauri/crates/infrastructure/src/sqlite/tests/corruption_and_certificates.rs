use super::*;

#[test]
fn schema_19_is_rejected_and_legacy_data_is_retained() {
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

    assert!(matches!(
        SqliteStore::open(&path),
        Err(InfrastructureError::DatabaseSchemaInvalid { .. })
    ));
    let connection = Connection::open(&path).expect("reopen legacy database");
    let version = connection
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("legacy schema marker");
    let legacy_exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'legacy_listener_policy')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("legacy table probe");
    let legacy_value = connection
        .query_row("SELECT value FROM legacy_listener_policy", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("legacy row");
    assert_eq!(version, 19);
    assert!(legacy_exists);
    assert_eq!(legacy_value, "obsolete");
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
