use super::super::*;
use super::{FakeRunner, RecordingRunner, SequenceRunner};
use intercept_proxy_application::{
    AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState,
    AndroidRuntimeOwnerTransitionReason, AndroidRuntimeOwnerViewModel,
};

fn owner(serial: &str, epoch: uuid::Uuid) -> AndroidRuntimeOwnerViewModel {
    AndroidRuntimeOwnerViewModel {
        serial: serial.into(),
        epoch,
        mode: AndroidRuntimeOwnerMode::AdbReverse,
        profile_id: "profile-a".into(),
        state: AndroidRuntimeOwnerState::Active,
        source: AndroidRuntimeOwnerSource::Start,
        transition_reason: AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        updated_at: chrono::Utc::now(),
    }
}

async fn seed_owner(adapter: &AndroidAdbAdapter, serial: &str, ports: Vec<u16>) -> uuid::Uuid {
    let epoch = uuid::Uuid::new_v4();
    *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
        epoch,
        serial: serial.into(),
        profile_id: "profile-a".into(),
        ports,
    });
    *adapter.active_runtime.lock().await = Some(ActiveRuntimeFacts {
        epoch,
        serial: serial.into(),
        profile_id: "profile-a".into(),
        profile_fingerprint: "profile".into(),
        route_fingerprint: "routes".into(),
        route_count: 1,
        listener_ports: BTreeMap::new(),
        uses_adb_reverse: true,
        endpoints: Vec::new(),
    });
    adapter.save_owner(owner(serial, epoch)).await.unwrap();
    epoch
}

#[tokio::test]
async fn selected_b_stop_targets_only_runtime_owner_a() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());
    seed_owner(&adapter, "DEVICE-A", vec![31_627]).await;

    let status = adapter.network_stop().await.unwrap();

    assert_eq!(status.serial, "DEVICE-A");
    assert!(adapter.runtime_owner_snapshot().await.is_none());
    {
        let calls = runner.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .all(|args| args.get(1).is_none_or(|serial| serial == "DEVICE-A"))
        );
        assert!(calls.iter().any(|args| args.ends_with(&[
            "reverse".into(),
            "--remove".into(),
            "tcp:31627".into()
        ])));
    }
    let repeated = adapter.network_stop().await.unwrap();
    assert_eq!(repeated.state, AndroidNetworkState::Stopped);
    assert!(repeated.verified);
    assert!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| { args.get(1).is_none_or(|serial| serial == "DEVICE-A") })
    );
}

#[tokio::test]
async fn no_owner_status_and_emergency_are_idempotent_without_selected_device_calls() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());

    let status = adapter.network_status().await.unwrap();
    let emergency = adapter.emergency_restore().await.unwrap();

    assert_eq!(status.state, AndroidNetworkState::Stopped);
    assert!(status.verified);
    assert_eq!(emergency.state, AndroidNetworkState::Stopped);
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unreachable_owner_never_cleans_reverse_or_targets_selected_device() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            AdbOutput {
                success: false,
                stdout: String::new(),
                stderr: "offline".into(),
            },
            AdbOutput {
                success: false,
                stdout: String::new(),
                stderr: "offline".into(),
            },
        ])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());
    let epoch = seed_owner(&adapter, "DEVICE-A", vec![31_627]).await;

    let error = adapter.network_stop().await.unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_ADB_COMMAND_FAILED");
    assert_eq!(adapter.required_runtime_owner().await.unwrap().epoch, epoch);
    assert_eq!(
        adapter.required_runtime_owner().await.unwrap().state,
        AndroidRuntimeOwnerState::StopFailed
    );
    assert_eq!(
        adapter.active_reverse.lock().await.as_ref().unwrap().ports,
        vec![31_627]
    );
    let calls = runner.calls.lock().unwrap();
    assert!(calls.iter().all(|args| args[1] == "DEVICE-A"));
    assert!(calls.iter().all(|args| !args.contains(&"--remove".into())));
}

#[tokio::test]
async fn status_disconnect_and_reconnect_are_owner_bound_and_persisted() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: false,
            stdout: String::new(),
            stderr: "offline".into(),
        }])),
    });
    let mut adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());
    let epoch = seed_owner(&adapter, "DEVICE-A", vec![31_627]).await;

    let disconnected = adapter.network_status().await.unwrap();

    assert_eq!(disconnected.serial, "DEVICE-A");
    assert_eq!(
        adapter.required_runtime_owner().await.unwrap().state,
        AndroidRuntimeOwnerState::WaitingReconnect
    );
    let stored = adapter
        .runtime_store
        .load_android_runtime_owner()
        .unwrap()
        .unwrap();
    assert_eq!(stored.resume_state, Some(AndroidRuntimeOwnerState::Active));
    assert!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| args[1] == "DEVICE-A")
    );

    let reconnected = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            AdbOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            },
            AdbOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            },
            AdbOutput {
                success: true,
                stdout: "1234\n".into(),
                stderr: String::new(),
            },
        ])),
    });
    adapter.runner = reconnected.clone();
    adapter.network_status().await.unwrap();

    let owner = adapter.required_runtime_owner().await.unwrap();
    assert_eq!(owner.epoch, epoch);
    assert_eq!(owner.state, AndroidRuntimeOwnerState::Uncertain);
    assert_eq!(
        owner.transition_reason,
        AndroidRuntimeOwnerTransitionReason::DeviceReconnected
    );
    assert!(
        reconnected
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| args[1] == "DEVICE-A")
    );
}

#[tokio::test]
async fn reconnect_classification_preserves_cleanup_and_stop_failure_states() {
    for protected in [
        AndroidRuntimeOwnerState::CleanupRequired,
        AndroidRuntimeOwnerState::StopFailed,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
        let epoch = seed_owner(&adapter, "DEVICE-A", vec![31_627]).await;
        let mut current = adapter.required_runtime_owner().await.unwrap();
        current.state = protected;
        adapter.save_owner(current).await.unwrap();
        adapter.mark_owner_waiting_reconnect(epoch).await.unwrap();

        adapter
            .mark_owner_reconnected(epoch, Some(AndroidNetworkState::Running))
            .await
            .unwrap();

        assert_eq!(
            adapter.required_runtime_owner().await.unwrap().state,
            protected
        );
    }

    for observed in [AndroidNetworkState::Stopped, AndroidNetworkState::Faulted] {
        let temp = tempfile::tempdir().unwrap();
        let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
        let epoch = seed_owner(&adapter, "DEVICE-A", vec![31_627]).await;
        adapter.mark_owner_waiting_reconnect(epoch).await.unwrap();
        adapter
            .mark_owner_reconnected(epoch, Some(observed))
            .await
            .unwrap();
        assert_eq!(
            adapter.required_runtime_owner().await.unwrap().state,
            AndroidRuntimeOwnerState::CleanupRequired
        );
    }
}

#[tokio::test]
async fn stale_epoch_cannot_clear_new_owner() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    let stale = uuid::Uuid::new_v4();
    let current = seed_owner(&adapter, "DEVICE-A", Vec::new()).await;

    assert!(!adapter.clear_owner_if_epoch(stale).await.unwrap());
    assert_eq!(
        adapter.required_runtime_owner().await.unwrap().epoch,
        current
    );
}

#[tokio::test]
async fn stale_reverse_epoch_is_not_persisted_with_new_owner() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    let stale_epoch = uuid::Uuid::new_v4();
    let current_epoch = uuid::Uuid::new_v4();
    *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
        epoch: stale_epoch,
        serial: "DEVICE-A".into(),
        profile_id: "profile-a".into(),
        ports: vec![31_627],
    });

    adapter
        .save_owner(owner("DEVICE-A", current_epoch))
        .await
        .unwrap();

    let stored = adapter
        .runtime_store
        .load_android_runtime_owner()
        .unwrap()
        .unwrap();
    assert_eq!(stored.owner.epoch, current_epoch);
    assert!(stored.reverse_ports.is_empty());
}

#[tokio::test]
async fn reopen_restores_owner_and_exact_reverse_ports_for_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("owner.sqlite3");
    let first_store = Arc::new(crate::SqliteStore::open(&path).unwrap());
    let first = AndroidAdbAdapter::new(None, first_store).unwrap();
    seed_owner(&first, "DEVICE-A", vec![31_627, 31_628]).await;
    drop(first);

    let store = Arc::new(crate::SqliteStore::open(&path).unwrap());
    let runner = Arc::new(RecordingRunner::default());
    let mut reopened = AndroidAdbAdapter::new(None, store).unwrap();
    reopened.adb_path = Some(PathBuf::from("adb"));
    reopened.runner = runner.clone();
    *reopened.selected_serial.write().unwrap() = Some("DEVICE-B".into());

    let restored = reopened.required_runtime_owner().await.unwrap();
    assert_eq!(restored.serial, "DEVICE-A");
    assert_eq!(restored.source, AndroidRuntimeOwnerSource::Recovery);
    assert_eq!(
        restored.transition_reason,
        AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage
    );
    reopened.network_stop().await.unwrap();

    assert!(reopened.runtime_owner_snapshot().await.is_none());
    let calls = runner.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .all(|args| args.get(1).is_none_or(|serial| serial == "DEVICE-A"))
    );
    for port in ["tcp:31627", "tcp:31628"] {
        assert!(
            calls.iter().any(|args| {
                args.ends_with(&["reverse".into(), "--remove".into(), port.into()])
            })
        );
    }
}
