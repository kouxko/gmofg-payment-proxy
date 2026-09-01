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
             INSERT INTO pre_baseline_sentinel(value) VALUES ('must remain');",
        )
        .expect("seed version 99 database");
}

#[test]
fn pre_schema100_startup_clears_legacy_data_and_recreates_schema100() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let connection = Connection::open(&path).expect("connection");
    seed_pre_baseline_database(&connection);
    drop(connection);

    drop(SqliteStore::open(&path).expect("pre-Schema100 must recreate current storage"));

    let connection = Connection::open(&path).expect("reopen recreated database");
    let marker = connection
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("Schema100 marker exists");
    let sentinel_exists = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_schema
                WHERE type = 'table' AND name = 'pre_baseline_sentinel'
            )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .expect("probe pre-Schema100 sentinel");
    assert_eq!(marker, 100);
    assert!(!sentinel_exists, "pre-Schema100 data must be deleted");
}

#[test]
fn clear_pre_baseline_database_removes_main_wal_and_shm_files() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let wal = sqlite_sidecar_path(&path, "-wal");
    let shm = sqlite_sidecar_path(&path, "-shm");
    for file in [&path, &wal, &shm] {
        std::fs::write(file, b"legacy").expect("seed SQLite artifact");
    }

    clear_pre_baseline_database(&path).expect("clear every legacy SQLite artifact");

    assert!(!path.exists());
    assert!(!wal.exists());
    assert!(!shm.exists());
}

#[test]
fn startup_ownership_prevents_waiting_prebaseline_opener_from_deleting_schema100() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let connection = Connection::open(&path).expect("connection");
    seed_pre_baseline_database(&connection);
    drop(connection);

    let startup_ownership = acquire_startup_ownership(&path).expect("startup ownership");
    STARTUP_OWNERSHIP_CONTENTION_OBSERVED.store(false, std::sync::atomic::Ordering::SeqCst);
    STARTUP_OWNERSHIP_CONTENTION_PROBE_ENABLED.store(true, std::sync::atomic::Ordering::SeqCst);
    let opener_path = path.clone();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let opener = std::thread::spawn(move || {
        let result = SqliteStore::open(&opener_path);
        finished_tx.send(result).expect("report opener result");
    });
    let contention_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !STARTUP_OWNERSHIP_CONTENTION_OBSERVED.load(std::sync::atomic::Ordering::SeqCst) {
        assert!(
            std::time::Instant::now() < contention_deadline,
            "the second opener must reach the held startup lock"
        );
        std::thread::yield_now();
    }

    clear_pre_baseline_database(&path).expect("first startup clears pre-Schema100 storage");
    let mut connection = Connection::open(&path).expect("current connection");
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("current transaction");
    initialize_current_schema(&transaction).expect("initialize Schema100");
    transaction
        .execute_batch(
            "CREATE TABLE startup_sentinel(value TEXT NOT NULL);
             INSERT INTO startup_sentinel(value) VALUES ('preserve');",
        )
        .expect("seed current sentinel");
    transaction.commit().expect("commit current database");
    drop(connection);
    drop(startup_ownership);

    finished_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("waiting opener completes")
        .expect("waiting opener preserves current database");
    opener.join().expect("join opener");
    STARTUP_OWNERSHIP_CONTENTION_PROBE_ENABLED.store(false, std::sync::atomic::Ordering::SeqCst);

    let connection = Connection::open(&path).expect("inspect current database");
    let sentinel = connection
        .query_row("SELECT value FROM startup_sentinel", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("current sentinel remains");
    assert_eq!(sentinel, "preserve");
}
