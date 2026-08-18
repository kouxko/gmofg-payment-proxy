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
