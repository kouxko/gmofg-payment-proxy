use super::*;

#[tokio::test]
async fn reverse_cleanup_for_device_a_never_targets_device_b() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    let mut owner_a = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    owner_a.mode = AndroidRuntimeOwnerMode::AdbReverse;
    let mut owner_b = runtime_owner("DEVICE-B", AndroidRuntimeOwnerState::Active);
    owner_b.mode = AndroidRuntimeOwnerMode::AdbReverse;
    adapter.save_owner(owner_a.clone()).await.unwrap();
    adapter.save_owner(owner_b.clone()).await.unwrap();
    {
        let mut states = adapter.owner_states.lock().await;
        states.get_mut("DEVICE-A").unwrap().active_reverse = Some(ActiveReverseOwnership {
            epoch: owner_a.epoch,
            serial: owner_a.serial.clone(),
            profile_id: owner_a.profile_id.clone(),
            ports: vec![40123],
        });
        states.get_mut("DEVICE-B").unwrap().active_reverse = Some(ActiveReverseOwnership {
            epoch: owner_b.epoch,
            serial: owner_b.serial.clone(),
            profile_id: owner_b.profile_id.clone(),
            ports: vec![40124],
        });
    }

    adapter.cleanup_owner_reverse(&owner_a).await.unwrap();

    {
        let calls = runner.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .all(|args| args.get(1).is_some_and(|serial| serial == "DEVICE-A"))
        );
    }
    assert_eq!(
        adapter
            .owner_state_snapshot_for("DEVICE-B")
            .await
            .active_reverse
            .unwrap()
            .ports,
        vec![40124]
    );
}

#[tokio::test]
async fn preparation_uses_explicit_serial_even_when_selection_changes() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());
    let activation = test_activation(
        "profile-a",
        "203.0.113.10",
        ListenerId::from_uuid(uuid::Uuid::new_v4()),
        16127,
    );

    let prepared = adapter
        .prepare_usb_proxy_runtime(
            "DEVICE-A",
            None,
            &activation,
            AndroidRuntimeOwnerSource::Start,
        )
        .await
        .unwrap();

    assert_eq!(prepared.owner.serial, "DEVICE-A");
    assert!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| { args.get(1).is_some_and(|serial| serial == "DEVICE-A") })
    );
}

#[tokio::test]
async fn successful_prepared_update_commits_active_owner_and_returns_value() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::CleanupRequired);
    let prepared = prepared_runtime(owner.clone());
    adapter.save_owner(owner.clone()).await.unwrap();

    let value = adapter
        .finish_prepared_network_update(prepared, Ok::<_, AppError>("committed"))
        .await
        .unwrap();

    assert_eq!(value, "committed");
    let committed = adapter
        .runtime_owner_snapshot_for("DEVICE-A")
        .await
        .unwrap();
    assert_eq!(committed.epoch, owner.epoch);
    assert_eq!(committed.state, AndroidRuntimeOwnerState::Active);
    assert_eq!(
        committed.transition_reason,
        AndroidRuntimeOwnerTransitionReason::ActivationConfirmed
    );
}

#[tokio::test]
async fn uncertain_prepared_update_keeps_owner_and_marks_error_retryable() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::CleanupRequired);
    let prepared = prepared_runtime(owner.clone());
    adapter.save_owner(owner.clone()).await.unwrap();

    let error = adapter
        .retain_uncertain_network_update::<()>(
            prepared,
            AppError::new("ANDROID_STATUS_UNCERTAIN", "status timed out"),
        )
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_STATUS_UNCERTAIN");
    assert!(error.view_model.retryable);
    let retained = adapter
        .runtime_owner_snapshot_for("DEVICE-A")
        .await
        .unwrap();
    assert_eq!(retained.epoch, owner.epoch);
    assert_eq!(retained.state, AndroidRuntimeOwnerState::Uncertain);
    assert_eq!(
        retained.transition_reason,
        AndroidRuntimeOwnerTransitionReason::ActivationUncertain
    );
}

#[tokio::test]
async fn failed_prepared_update_without_reverse_restores_empty_owner_slot() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::CleanupRequired);
    let prepared = prepared_runtime(owner.clone());
    adapter.save_owner(owner).await.unwrap();

    let error = adapter
        .finish_prepared_network_update::<()>(
            prepared,
            Err(AppError::new("ANDROID_APPLY_FAILED", "apply failed")),
        )
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_APPLY_FAILED");
    assert!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .is_none()
    );
}

#[tokio::test]
async fn failed_reverse_rollback_persists_cleanup_required_ports() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: false,
            stdout: String::new(),
            stderr: "permission denied".into(),
        }])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner);
    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::CleanupRequired);
    let mut prepared = prepared_runtime(owner.clone());
    prepared.reverse = Some(ActiveReverseOwnership {
        epoch: owner.epoch,
        serial: owner.serial.clone(),
        profile_id: owner.profile_id.clone(),
        ports: vec![40_123],
    });
    adapter.save_owner(owner.clone()).await.unwrap();

    let error = adapter
        .finish_prepared_network_update::<()>(
            prepared,
            Err(AppError::new("ANDROID_APPLY_FAILED", "apply failed")),
        )
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_APPLY_FAILED");
    assert!(error.view_model.message.contains("reverse"));
    let state = adapter.owner_state_snapshot_for("DEVICE-A").await;
    assert_eq!(
        state.runtime_owner.unwrap().state,
        AndroidRuntimeOwnerState::CleanupRequired
    );
    assert_eq!(state.active_reverse.unwrap().ports, vec![40_123]);
}

#[tokio::test]
async fn failed_previous_reverse_cleanup_commits_new_and_retains_old_ports() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: false,
            stdout: String::new(),
            stderr: "permission denied".into(),
        }])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner);
    let previous = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    let mut owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::CleanupRequired);
    owner.epoch = previous.epoch;
    let mut prepared = prepared_runtime(owner.clone());
    prepared.reverse = Some(ActiveReverseOwnership {
        epoch: owner.epoch,
        serial: owner.serial.clone(),
        profile_id: owner.profile_id.clone(),
        ports: vec![40_124],
    });
    prepared.previous_reverse = Some(ActiveReverseOwnership {
        epoch: previous.epoch,
        serial: previous.serial.clone(),
        profile_id: previous.profile_id.clone(),
        ports: vec![40_123],
    });
    adapter.save_owner(previous).await.unwrap();

    let error = adapter
        .finish_prepared_network_update(prepared, Ok::<_, AppError>(()))
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_ADB_REVERSE_CLEANUP_FAILED");
    let state = adapter.owner_state_snapshot_for("DEVICE-A").await;
    assert_eq!(
        state.runtime_owner.unwrap().state,
        AndroidRuntimeOwnerState::CleanupRequired
    );
    assert_eq!(state.active_reverse.unwrap().ports, vec![40_123, 40_124]);
}

#[tokio::test]
async fn cleanup_without_cached_reverse_removes_all_for_reverse_owner() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    let mut owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    owner.mode = AndroidRuntimeOwnerMode::AdbReverse;
    adapter.save_owner(owner.clone()).await.unwrap();

    adapter.cleanup_owner_reverse(&owner).await.unwrap();

    assert!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args == &["-s", "DEVICE-A", "reverse", "--remove-all"])
    );
}

#[tokio::test]
async fn cleanup_failure_retains_only_failed_reverse_ports() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            AdbOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            },
            AdbOutput {
                success: false,
                stdout: String::new(),
                stderr: "permission denied".into(),
            },
        ])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner);
    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    adapter.save_owner(owner.clone()).await.unwrap();
    {
        let mut states = adapter.owner_states.lock().await;
        states.get_mut("DEVICE-A").unwrap().active_reverse = Some(ActiveReverseOwnership {
            epoch: owner.epoch,
            serial: owner.serial.clone(),
            profile_id: owner.profile_id.clone(),
            ports: vec![40_123, 40_124],
        });
    }

    let error = adapter.cleanup_owner_reverse(&owner).await.unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_ADB_REVERSE_CLEANUP_FAILED");
    assert_eq!(
        adapter
            .owner_state_snapshot_for("DEVICE-A")
            .await
            .active_reverse
            .unwrap()
            .ports,
        vec![40_124]
    );
}

#[tokio::test]
async fn preparation_rejects_missing_and_stale_expected_epoch_before_adb() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    let activation = test_activation(
        "profile-a",
        "203.0.113.10",
        ListenerId::from_uuid(uuid::Uuid::new_v4()),
        16_127,
    );

    let missing = adapter
        .prepare_usb_proxy_runtime(
            "DEVICE-A",
            Some(uuid::Uuid::new_v4()),
            &activation,
            AndroidRuntimeOwnerSource::Apply,
        )
        .await
        .unwrap_err();
    assert_eq!(missing.view_model.code, "ANDROID_RUNTIME_NOT_MANAGED");

    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    adapter.save_owner(owner.clone()).await.unwrap();
    let stale = adapter
        .prepare_usb_proxy_runtime(
            "DEVICE-A",
            Some(uuid::Uuid::new_v4()),
            &activation,
            AndroidRuntimeOwnerSource::Apply,
        )
        .await
        .unwrap_err();
    assert_eq!(stale.view_model.code, "ANDROID_RUNTIME_EPOCH_STALE");
    assert_eq!(stale.view_model.runtime_epoch, Some(owner.epoch));
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn first_reverse_creation_failure_restores_empty_owner_slot() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            AdbOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            },
            AdbOutput {
                success: false,
                stdout: String::new(),
                stderr: "reverse unavailable".into(),
            },
        ])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner);
    let activation = test_activation(
        "profile-a",
        "203.0.113.10",
        ListenerId::from_uuid(uuid::Uuid::new_v4()),
        16_127,
    );

    let error = adapter
        .prepare_usb_proxy_runtime(
            "DEVICE-A",
            None,
            &activation,
            AndroidRuntimeOwnerSource::Start,
        )
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_ADB_REVERSE_CREATE_FAILED");
    assert!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .is_none()
    );
    assert!(
        adapter
            .runtime_store
            .load_android_runtime_owners()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn partial_reverse_creation_failure_retains_uncleaned_port() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
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
                success: false,
                stdout: String::new(),
                stderr: "second reverse unavailable".into(),
            },
            AdbOutput {
                success: false,
                stdout: String::new(),
                stderr: "cleanup denied".into(),
            },
        ])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner);
    let mut activation = test_activation(
        "profile-a",
        "203.0.113.10",
        ListenerId::from_uuid(uuid::Uuid::new_v4()),
        16_127,
    );
    let mut second_route = activation.proxy_routes[0].clone();
    second_route.listener_id = uuid::Uuid::new_v4().to_string();
    second_route.listener_name = "Second listener".into();
    second_route.desktop_listener_port = 26_127;
    activation.proxy_routes.push(second_route);

    let error = adapter
        .prepare_usb_proxy_runtime(
            "DEVICE-A",
            None,
            &activation,
            AndroidRuntimeOwnerSource::Start,
        )
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_ADB_REVERSE_CREATE_FAILED");
    assert!(error.view_model.message.contains("清理失败"));
    let state = adapter.owner_state_snapshot_for("DEVICE-A").await;
    let retained = state.runtime_owner.unwrap();
    assert_eq!(retained.state, AndroidRuntimeOwnerState::CleanupRequired);
    assert_eq!(
        retained.transition_reason,
        AndroidRuntimeOwnerTransitionReason::ReverseCleanupRequired
    );
    assert_eq!(state.active_reverse.unwrap().ports.len(), 1);
}
