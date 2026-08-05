use super::*;

/// RULE-001, SECURITY-004: rule import replaces the collection in one
/// transaction.
#[test]
fn rule_import_is_atomic() {
    let store = SqliteStore::in_memory().expect("store");
    let record = RuleRecord {
        id: Uuid::new_v4(),
        revision: 1,
        enabled: true,
        value: json!({"name": "first"}),
        updated_at: Utc::now(),
    };
    store
        .replace_rules_atomically(0, std::slice::from_ref(&record))
        .expect("replace");
    assert_eq!(store.list_rules().expect("list"), vec![record.clone()]);

    let duplicate = RuleRecord {
        id: Uuid::new_v4(),
        revision: 1,
        enabled: true,
        value: json!({"name": "duplicate"}),
        updated_at: Utc::now(),
    };
    assert!(matches!(
        store.replace_rules_atomically(1, &[duplicate.clone(), duplicate]),
        Err(InfrastructureError::RevisionConflict)
    ));
    let snapshot = store
        .load_rules_snapshot()
        .expect("snapshot after rollback");
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.records, vec![record]);
}

#[test]
fn independent_stores_preserve_unrelated_rules_and_reject_stale_writes() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("rules.sqlite3");
    let first_store = Arc::new(SqliteStore::open(&path).expect("first store"));
    let second_store = Arc::new(SqliteStore::open(&path).expect("second store"));
    let barrier = Arc::new(Barrier::new(3));
    let first = RuleRecord {
        id: Uuid::new_v4(),
        revision: 1,
        enabled: true,
        value: json!({"name": "first", "revision": 1}),
        updated_at: Utc::now(),
    };
    let second = RuleRecord {
        id: Uuid::new_v4(),
        revision: 1,
        enabled: true,
        value: json!({"name": "second", "revision": 1}),
        updated_at: Utc::now(),
    };

    let first_insert = {
        let store = Arc::clone(&first_store);
        let barrier = Arc::clone(&barrier);
        let record = first.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.insert_rule(&record)
        })
    };
    let second_insert = {
        let store = Arc::clone(&second_store);
        let barrier = Arc::clone(&barrier);
        let record = second.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.insert_rule(&record)
        })
    };
    barrier.wait();
    first_insert
        .join()
        .expect("first thread")
        .expect("first insert");
    second_insert
        .join()
        .expect("second thread")
        .expect("second insert");

    let snapshot = first_store.load_rules_snapshot().expect("snapshot");
    assert_eq!(snapshot.revision, 2);
    assert_eq!(snapshot.records.len(), 2);
    assert!(snapshot.records.iter().any(|record| record.id == first.id));
    assert!(snapshot.records.iter().any(|record| record.id == second.id));

    let mut winner = first.clone();
    winner.revision = 2;
    winner.value = json!({"name": "winner", "revision": 2});
    second_store
        .compare_and_swap_rule(1, &winner)
        .expect("winning update");

    let mut stale = first;
    stale.revision = 2;
    stale.value = json!({"name": "stale", "revision": 2});
    assert!(matches!(
        first_store.compare_and_swap_rule(1, &stale),
        Err(InfrastructureError::RevisionConflict)
    ));

    first_store
        .delete_rule(second.id, 1)
        .expect("delete unrelated rule");
    assert!(matches!(
        second_store.delete_rule(second.id, 1),
        Err(InfrastructureError::RevisionConflict)
    ));
    let after_delete = first_store.list_rules().expect("rules after delete");
    assert_eq!(after_delete.len(), 1);
    assert_eq!(after_delete[0].id, winner.id);

    let stale_collection_revision = first_store
        .load_rules_snapshot()
        .expect("pre-insert snapshot")
        .revision;
    let third = RuleRecord {
        id: Uuid::new_v4(),
        revision: 1,
        enabled: true,
        value: json!({"name": "third", "revision": 1}),
        updated_at: Utc::now(),
    };
    second_store.insert_rule(&third).expect("third insert");
    assert!(matches!(
        first_store.replace_rules_atomically(stale_collection_revision, &snapshot.records),
        Err(InfrastructureError::RevisionConflict)
    ));
    let final_rules = first_store.list_rules().expect("final rules");
    assert_eq!(final_rules.len(), 2);
    assert!(final_rules.iter().any(|record| record.id == winner.id));
    assert!(final_rules.iter().any(|record| record.id == third.id));
}

#[test]
fn concurrent_runtime_hits_conflict_then_retry_without_lost_updates() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("runtime-rules.sqlite3");
    let first_store = Arc::new(SqliteStore::open(&path).expect("first store"));
    let second_store = Arc::new(SqliteStore::open(&path).expect("second store"));
    let rule_id = Uuid::new_v4();
    first_store
        .insert_rule(&RuleRecord {
            id: rule_id,
            revision: 1,
            enabled: true,
            value: json!({
                "revision": 1,
                "enabled": true,
                "hit_count": 0,
                "last_hit_at": null
            }),
            updated_at: Utc::now(),
        })
        .expect("seed rule");
    let barrier = Arc::new(Barrier::new(3));
    let writers = [Arc::clone(&first_store), Arc::clone(&second_store)].map(|store| {
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            store.compare_and_swap_rule_runtime(
                1,
                &[(rule_id, 1)],
                &[RuleRuntimeUpdate {
                    id: rule_id,
                    expected_revision: 1,
                    revision: 1,
                    enabled: true,
                    hit_count: 1,
                    last_hit_at: None,
                }],
            )
        })
    });
    barrier.wait();
    let results = writers.map(|writer| writer.join().expect("writer thread"));
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(InfrastructureError::RevisionConflict)))
            .count(),
        1
    );

    let current = first_store.load_rules_snapshot().expect("current snapshot");
    assert_eq!(current.revision, 2);
    assert_eq!(
        current.records[0]
            .value
            .get("hit_count")
            .and_then(Value::as_u64),
        Some(1)
    );
    first_store
        .compare_and_swap_rule_runtime(
            current.revision,
            &[(rule_id, 1)],
            &[RuleRuntimeUpdate {
                id: rule_id,
                expected_revision: 1,
                revision: 1,
                enabled: true,
                hit_count: 2,
                last_hit_at: None,
            }],
        )
        .expect("retry from refreshed snapshot");
    let final_snapshot = second_store.load_rules_snapshot().expect("final snapshot");
    assert_eq!(final_snapshot.revision, 3);
    assert_eq!(
        final_snapshot.records[0]
            .value
            .get("hit_count")
            .and_then(Value::as_u64),
        Some(2)
    );
}
