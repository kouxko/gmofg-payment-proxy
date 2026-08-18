use intercept_proxy_application::{
    AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState,
    AndroidRuntimeOwnerTransitionReason, AndroidRuntimeOwnerViewModel,
};

use super::*;

fn record(epoch: Uuid) -> AndroidRuntimeOwnerRecord {
    AndroidRuntimeOwnerRecord {
        owner: AndroidRuntimeOwnerViewModel {
            serial: "DEVICE-A".into(),
            epoch,
            mode: AndroidRuntimeOwnerMode::AdbReverse,
            profile_id: "profile-a".into(),
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
fn owner_and_cleanup_ports_survive_reopen_but_stale_epoch_cannot_clear() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.sqlite");
    let epoch = Uuid::new_v4();
    let expected = record(epoch);
    SqliteStore::open(&path)
        .unwrap()
        .save_android_runtime_owner(&expected)
        .unwrap();

    let reopened = SqliteStore::open(&path).unwrap();
    assert_eq!(
        reopened.load_android_runtime_owner().unwrap(),
        Some(expected)
    );
    assert!(
        !reopened
            .clear_android_runtime_owner(Uuid::new_v4())
            .unwrap()
    );
    assert!(reopened.load_android_runtime_owner().unwrap().is_some());
    assert!(reopened.clear_android_runtime_owner(epoch).unwrap());
    assert!(reopened.load_android_runtime_owner().unwrap().is_none());
}

#[test]
fn application_data_reset_removes_local_runtime_owner() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .save_android_runtime_owner(&record(Uuid::new_v4()))
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

    assert!(store.load_android_runtime_owner().unwrap().is_none());
}

#[test]
fn failed_owner_migration_rolls_back_owner_table_and_latest_version() {
    let store = SqliteStore::in_memory().unwrap();
    {
        let connection = store.connection.lock();
        connection
            .execute_batch(
                "DROP TABLE android_runtime_owner;
                 DELETE FROM schema_migrations WHERE version = 10;
                 CREATE TRIGGER reject_owner_migration
                 BEFORE INSERT ON schema_migrations WHEN NEW.version = 10
                 BEGIN SELECT RAISE(ABORT, 'reject owner migration'); END;",
            )
            .unwrap();
    }

    assert!(matches!(
        store.migrate(),
        Err(InfrastructureError::DatabaseMigration { .. })
    ));
    assert!(
        !store
            .table_names()
            .unwrap()
            .contains(&"android_runtime_owner".into())
    );
    let version: i64 = store
        .connection
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 10",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 0);
}

#[test]
fn version_eight_owner_row_migrates_atomically_with_resume_state_default() {
    let store = SqliteStore::in_memory().unwrap();
    let epoch = Uuid::new_v4();
    store
        .execute_test_batch(&format!(
            "DROP TABLE android_runtime_owner;
             DELETE FROM schema_migrations WHERE version = 10;
             CREATE TABLE android_runtime_owner (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                serial TEXT NOT NULL, epoch TEXT NOT NULL,
                mode TEXT NOT NULL CHECK(mode IN ('device_only', 'lan', 'adb_reverse')),
                profile_id TEXT NOT NULL,
                state TEXT NOT NULL CHECK(state IN ('active', 'uncertain', 'stop_failed')),
                source TEXT NOT NULL CHECK(source IN ('start', 'apply', 'recovery')),
                transition_reason TEXT NOT NULL CHECK(transition_reason IN (
                    'activation_confirmed', 'activation_uncertain', 'stop_failed',
                    'recovered_from_storage'
                )),
                reverse_ports_json TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             INSERT INTO android_runtime_owner VALUES (
                1, 'DEVICE-A', '{epoch}', 'adb_reverse', 'profile-a', 'active', 'start',
                'activation_confirmed', '[31627]', '2026-08-17T00:00:00Z'
             );"
        ))
        .unwrap();

    store.migrate().unwrap();

    let loaded = store.load_android_runtime_owner().unwrap().unwrap();
    assert_eq!(loaded.owner.epoch, epoch);
    assert_eq!(loaded.reverse_ports, vec![31_627]);
    assert_eq!(loaded.resume_state, None);
    assert!(loaded.runtime_endpoints.is_empty());
}

#[test]
fn owner_enum_round_trips_and_epoch_replace_is_compare_and_swap() {
    let store = SqliteStore::in_memory().unwrap();
    let epoch = Uuid::new_v4();
    let mut value = record(epoch);
    for mode in [
        AndroidRuntimeOwnerMode::DeviceOnly,
        AndroidRuntimeOwnerMode::Lan,
        AndroidRuntimeOwnerMode::AdbReverse,
    ] {
        value.owner.mode = mode;
        store.save_android_runtime_owner(&value).unwrap();
        assert_eq!(
            store.load_android_runtime_owner().unwrap(),
            Some(value.clone())
        );
    }
    for source in [
        AndroidRuntimeOwnerSource::Start,
        AndroidRuntimeOwnerSource::Apply,
        AndroidRuntimeOwnerSource::Recovery,
    ] {
        value.owner.source = source;
        store.save_android_runtime_owner(&value).unwrap();
        assert_eq!(
            store.load_android_runtime_owner().unwrap(),
            Some(value.clone())
        );
    }
    let states = [
        AndroidRuntimeOwnerState::Active,
        AndroidRuntimeOwnerState::Uncertain,
        AndroidRuntimeOwnerState::WaitingReconnect,
        AndroidRuntimeOwnerState::CleanupRequired,
        AndroidRuntimeOwnerState::StopFailed,
        AndroidRuntimeOwnerState::Faulted,
    ];
    for state in states {
        value.owner.state = state;
        value.resume_state = Some(state);
        store.save_android_runtime_owner(&value).unwrap();
        assert_eq!(
            store.load_android_runtime_owner().unwrap(),
            Some(value.clone())
        );
    }
    let reasons = [
        AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        AndroidRuntimeOwnerTransitionReason::ActivationUncertain,
        AndroidRuntimeOwnerTransitionReason::ReversePreparation,
        AndroidRuntimeOwnerTransitionReason::ReverseCleanupRequired,
        AndroidRuntimeOwnerTransitionReason::DeviceDisconnected,
        AndroidRuntimeOwnerTransitionReason::DeviceReconnected,
        AndroidRuntimeOwnerTransitionReason::StopFailed,
        AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage,
        AndroidRuntimeOwnerTransitionReason::LanEndpointReapplied,
        AndroidRuntimeOwnerTransitionReason::LanEndpointFaulted,
    ];
    for reason in reasons {
        value.owner.transition_reason = reason;
        store.save_android_runtime_owner(&value).unwrap();
        assert_eq!(
            store.load_android_runtime_owner().unwrap(),
            Some(value.clone())
        );
    }
    assert!(
        !store
            .replace_android_runtime_owner_if_epoch(Uuid::new_v4(), &value)
            .unwrap()
    );
    assert!(
        store
            .replace_android_runtime_owner_if_epoch(epoch, &value)
            .unwrap()
    );
}
