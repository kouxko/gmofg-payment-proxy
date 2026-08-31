use std::{sync::Arc, thread};

use intercept_proxy_application::{
    AndroidRuntimeEndpointHealth, AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerMode,
    AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason,
    AndroidRuntimeOwnerViewModel,
};

use super::*;

fn record(serial: &str, epoch: Uuid) -> AndroidRuntimeOwnerRecord {
    AndroidRuntimeOwnerRecord {
        owner: AndroidRuntimeOwnerViewModel {
            serial: serial.into(),
            epoch,
            mode: AndroidRuntimeOwnerMode::AdbReverse,
            profile_id: format!("profile-{serial}"),
            state: AndroidRuntimeOwnerState::Active,
            source: AndroidRuntimeOwnerSource::Start,
            transition_reason: AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
            updated_at: Utc::now(),
        },
        reverse_ports: vec![31_627, 31_628],
        resume_state: None,
        runtime_endpoints: Vec::new(),
    }
}

#[test]
fn owners_survive_reopen_and_load_in_serial_order() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    let mut records = [
        record("DEVICE-C", Uuid::new_v4()),
        record("DEVICE-A", Uuid::new_v4()),
        record("DEVICE-B", Uuid::new_v4()),
    ];
    records[0].resume_state = Some(AndroidRuntimeOwnerState::Uncertain);
    records[0].runtime_endpoints = vec![runtime_endpoint(&records[0].owner)];
    let store = SqliteStore::open(&path).unwrap();
    for value in &records {
        store.reserve_android_runtime_owner(value).unwrap();
    }
    drop(store);

    let loaded = SqliteStore::open(&path)
        .unwrap()
        .load_android_runtime_owners()
        .unwrap();
    assert_eq!(
        loaded
            .iter()
            .map(|value| value.owner.serial.as_str())
            .collect::<Vec<_>>(),
        vec!["DEVICE-A", "DEVICE-B", "DEVICE-C"]
    );
    for expected in records {
        assert!(loaded.contains(&expected));
    }
}

fn runtime_endpoint(owner: &AndroidRuntimeOwnerViewModel) -> AndroidRuntimeEndpointViewModel {
    AndroidRuntimeEndpointViewModel {
        serial: owner.serial.clone(),
        epoch: owner.epoch,
        mode: owner.mode,
        original_destination: "payments.example.test".into(),
        original_ports: vec![443],
        resolved_original_ips: vec!["192.0.2.10".into()],
        listener_id: "listener-a".into(),
        listener_name: "Listener A".into(),
        desktop_listener_port: 8443,
        proxy_host: "10.0.0.2".into(),
        proxy_port: 8443,
        resolved_at: Utc::now(),
        health: AndroidRuntimeEndpointHealth::WaitingReconnect,
    }
}

#[test]
fn clear_and_replace_are_scoped_by_serial_and_expected_epoch() {
    let store = SqliteStore::in_memory().unwrap();
    let epoch_a = Uuid::new_v4();
    let owner_a = record("DEVICE-A", epoch_a);
    let owner_b = record("DEVICE-B", Uuid::new_v4());
    store.reserve_android_runtime_owner(&owner_a).unwrap();
    store.reserve_android_runtime_owner(&owner_b).unwrap();

    let successor_epoch = Uuid::new_v4();
    let mut successor = record("DEVICE-A", successor_epoch);
    successor.owner.source = AndroidRuntimeOwnerSource::Apply;
    assert!(
        !store
            .replace_android_runtime_owner_if_epoch("DEVICE-A", Uuid::new_v4(), &successor,)
            .unwrap()
    );
    assert!(
        store
            .replace_android_runtime_owner_if_epoch("DEVICE-A", epoch_a, &successor)
            .unwrap()
    );
    assert!(
        !store
            .clear_android_runtime_owner("DEVICE-A", epoch_a)
            .unwrap()
    );
    assert!(
        store
            .clear_android_runtime_owner("DEVICE-A", successor_epoch)
            .unwrap()
    );
    assert_eq!(store.load_android_runtime_owners().unwrap(), vec![owner_b]);
}

#[test]
fn replace_rejects_a_record_for_another_serial_without_mutation() {
    let store = SqliteStore::in_memory().unwrap();
    let epoch = Uuid::new_v4();
    let original = record("DEVICE-A", epoch);
    store.reserve_android_runtime_owner(&original).unwrap();

    assert!(
        store
            .replace_android_runtime_owner_if_epoch(
                "DEVICE-A",
                epoch,
                &record("DEVICE-B", Uuid::new_v4()),
            )
            .is_err()
    );
    assert_eq!(store.load_android_runtime_owners().unwrap(), vec![original]);
}

#[test]
fn epoch_is_unique_across_serials() {
    let store = SqliteStore::in_memory().unwrap();
    let epoch = Uuid::new_v4();
    store
        .reserve_android_runtime_owner(&record("DEVICE-A", epoch))
        .unwrap();
    assert!(
        store
            .reserve_android_runtime_owner(&record("DEVICE-B", epoch))
            .is_err()
    );
    assert_eq!(store.load_android_runtime_owners().unwrap().len(), 1);
}

#[test]
fn capacity_allows_eight_and_rejects_ninth_without_changing_snapshot() {
    let store = SqliteStore::in_memory().unwrap();
    for index in 0..8 {
        store
            .reserve_android_runtime_owner(&record(&format!("DEVICE-{index}"), Uuid::new_v4()))
            .unwrap();
    }
    let before = store.load_android_runtime_owners().unwrap();
    assert!(
        store
            .reserve_android_runtime_owner(&record("DEVICE-8", Uuid::new_v4()))
            .is_err()
    );
    assert_eq!(store.load_android_runtime_owners().unwrap(), before);
}

#[test]
fn full_capacity_still_allows_epoch_guarded_update_of_existing_serial() {
    let store = SqliteStore::in_memory().unwrap();
    let original_epoch = Uuid::new_v4();
    store
        .reserve_android_runtime_owner(&record("DEVICE-0", original_epoch))
        .unwrap();
    for index in 1..8 {
        store
            .reserve_android_runtime_owner(&record(&format!("DEVICE-{index}"), Uuid::new_v4()))
            .unwrap();
    }
    let mut replacement = record("DEVICE-0", Uuid::new_v4());
    replacement.owner.state = AndroidRuntimeOwnerState::WaitingReconnect;

    assert!(
        store
            .replace_android_runtime_owner_if_epoch("DEVICE-0", original_epoch, &replacement)
            .unwrap()
    );
    assert_eq!(store.load_android_runtime_owners().unwrap().len(), 8);
    assert_eq!(store.load_android_runtime_owners().unwrap()[0], replacement);
}

#[test]
fn one_corrupt_row_fails_the_whole_collection_load() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .reserve_android_runtime_owner(&record("DEVICE-A", Uuid::new_v4()))
        .unwrap();
    store
        .reserve_android_runtime_owner(&record("DEVICE-B", Uuid::new_v4()))
        .unwrap();
    store
        .execute_test_batch(
            "UPDATE android_runtime_owners SET epoch = 'not-a-uuid' WHERE serial = 'DEVICE-B';",
        )
        .unwrap();

    assert!(store.load_android_runtime_owners().is_err());
}

#[test]
fn concurrent_admission_from_seven_commits_exactly_one_new_serial() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    let seed = SqliteStore::open(&path).unwrap();
    for index in 0..7 {
        seed.reserve_android_runtime_owner(&record(&format!("DEVICE-{index}"), Uuid::new_v4()))
            .unwrap();
    }
    drop(seed);

    let barrier = Arc::new(std::sync::Barrier::new(3));
    let handles = ["DEVICE-A", "DEVICE-B"].map(|serial| {
        let path = path.clone();
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            let store = SqliteStore::open(&path).unwrap();
            barrier.wait();
            store
                .reserve_android_runtime_owner(&record(serial, Uuid::new_v4()))
                .is_ok()
        })
    });
    barrier.wait();
    let successes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|success| *success)
        .count();

    assert_eq!(successes, 1);
    assert_eq!(
        SqliteStore::open(&path)
            .unwrap()
            .load_android_runtime_owners()
            .unwrap()
            .len(),
        8
    );
}

#[test]
fn database_trigger_rejects_direct_ninth_insert() {
    let store = SqliteStore::in_memory().unwrap();
    for index in 0..8 {
        store
            .reserve_android_runtime_owner(&record(&format!("DEVICE-{index}"), Uuid::new_v4()))
            .unwrap();
    }
    let error = store
        .execute_test_batch(
            "INSERT INTO android_runtime_owners(
                serial, epoch, mode, profile_id, state, source, transition_reason,
                reverse_ports_json, resume_state, runtime_endpoints_json, updated_at
             ) VALUES (
                'DEVICE-8', '00000000-0000-0000-0000-000000000008', 'device_only',
                'profile-8', 'active', 'start', 'activation_confirmed', '[]', NULL, '[]',
                '2026-08-27T00:00:00Z'
             );",
        )
        .unwrap_err();
    assert!(format!("{error:?}").contains("ANDROID_RUNTIME_CAPACITY_EXCEEDED"));
}

#[test]
fn capacity_trigger_does_not_reject_same_serial_upsert_when_full() {
    let store = SqliteStore::in_memory().unwrap();
    for index in 0..8 {
        store
            .reserve_android_runtime_owner(&record(&format!("DEVICE-{index}"), Uuid::new_v4()))
            .unwrap();
    }

    store
        .execute_test_batch(
            "INSERT INTO android_runtime_owners(
                serial, epoch, mode, profile_id, state, source, transition_reason,
                reverse_ports_json, resume_state, runtime_endpoints_json, updated_at
             ) VALUES (
                'DEVICE-0', '00000000-0000-0000-0000-000000000080', 'device_only',
                'profile-0', 'active', 'start', 'activation_confirmed', '[]', NULL, '[]',
                '2026-08-27T00:00:00Z'
             ) ON CONFLICT(serial) DO UPDATE SET updated_at = excluded.updated_at;",
        )
        .unwrap();
    assert_eq!(store.load_android_runtime_owners().unwrap().len(), 8);
}

#[test]
fn version_twenty_singleton_schema_is_rejected_without_modifying_owner_data() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.sqlite");
    let legacy_epoch = Uuid::new_v4();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE application_schema(
                singleton_id INTEGER PRIMARY KEY, version INTEGER NOT NULL
             );
             INSERT INTO application_schema VALUES (1, 20);
             CREATE TABLE android_runtime_owner(
                singleton_id INTEGER PRIMARY KEY, serial TEXT NOT NULL, epoch TEXT NOT NULL
             );
             INSERT INTO android_runtime_owner VALUES (1, 'LEGACY', '{legacy_epoch}');"
        ))
        .unwrap();
    drop(connection);
    let before = std::fs::read(&path).expect("read legacy owner database before rejection");

    SqliteStore::open(&path).expect_err("version 20 owner schema must fail closed");
    assert_eq!(
        std::fs::read(&path).expect("read legacy owner database after rejection"),
        before
    );

    let connection = Connection::open(&path).unwrap();
    let version: i64 = connection
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let legacy_table_exists: bool = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'android_runtime_owner'
            )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let legacy_owner: (String, String) = connection
        .query_row(
            "SELECT serial, epoch FROM android_runtime_owner WHERE singleton_id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(version, 20);
    assert!(legacy_table_exists);
    assert_eq!(
        legacy_owner,
        ("LEGACY".to_owned(), legacy_epoch.to_string())
    );
}

#[test]
fn application_data_reset_removes_all_runtime_owners() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .reserve_android_runtime_owner(&record("DEVICE-A", Uuid::new_v4()))
        .unwrap();
    store
        .reserve_android_runtime_owner(&record("DEVICE-B", Uuid::new_v4()))
        .unwrap();
    let clean_id = Uuid::new_v4();
    let clean = WorkspaceRecord {
        id: clean_id,
        revision: 1,
        value: serde_json::json!({"id": clean_id, "name": "Default", "revision": 1}),
        updated_at: Utc::now(),
    };

    store
        .reset_application_data(clean_id, &[clean], &serde_json::json!({}))
        .unwrap();

    assert!(store.load_android_runtime_owners().unwrap().is_empty());
}
