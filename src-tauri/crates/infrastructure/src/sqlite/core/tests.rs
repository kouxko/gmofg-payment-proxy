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
fn preserve_only_startup_rejects_pre_schema100_without_modifying_it() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("state.sqlite");
    let connection = Connection::open(&path).expect("connection");
    seed_pre_baseline_database(&connection);
    drop(connection);

    SqliteStore::open(&path).expect_err("pre-Schema100 must fail closed");

    let connection = Connection::open(&path).expect("reopen rejected database");
    let marker = connection
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("pre-Schema100 marker remains");
    let sentinel = connection
        .query_row("SELECT value FROM pre_baseline_sentinel", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("pre-Schema100 sentinel remains");
    assert_eq!(marker, 99);
    assert_eq!(sentinel, "must remain");
}
