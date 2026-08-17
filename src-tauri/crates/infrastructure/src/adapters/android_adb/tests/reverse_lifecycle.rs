use super::super::*;

#[tokio::test]
async fn reverse_cleanup_uses_the_device_that_created_the_mapping() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());
    *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
        epoch: uuid::Uuid::new_v4(),
        serial: "DEVICE-A".into(),
        profile_id: "profile-a".into(),
        ports: vec![31_627],
    });

    adapter.clear_active_reverse_ports().await.unwrap();

    assert_eq!(
        *runner.calls.lock().unwrap(),
        vec![vec!["-s", "DEVICE-A", "reverse", "--remove", "tcp:31627",]]
    );
    assert!(adapter.active_reverse.lock().await.is_none());
}

#[tokio::test]
async fn owner_cleanup_persists_partial_failure_and_fallback_remove_all() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: false,
            stdout: String::new(),
            stderr: "cannot remove".into(),
        }])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner);
    let epoch = seed_active_runtime(&adapter, "DEVICE-A", vec![31_627])
        .await
        .0
        .epoch;
    let owner = adapter.required_runtime_owner().await.unwrap();

    adapter.cleanup_owner_reverse(&owner).await.unwrap_err();

    assert_eq!(
        adapter
            .runtime_store
            .load_android_runtime_owner()
            .unwrap()
            .unwrap()
            .reverse_ports,
        vec![31_627]
    );
    assert_eq!(adapter.required_runtime_owner().await.unwrap().epoch, epoch);

    let fallback = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), fallback.clone());
    let mut owner = owner;
    owner.epoch = uuid::Uuid::new_v4();
    adapter.save_owner(owner.clone()).await.unwrap();
    *adapter.active_reverse.lock().await = None;
    adapter.cleanup_owner_reverse(&owner).await.unwrap();
    assert!(
        fallback
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| { args.ends_with(&["reverse".into(), "--remove-all".into()]) })
    );
}

#[tokio::test]
async fn empty_active_reverse_cleanup_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    *adapter.active_runtime.lock().await = Some(activation_runtime());

    adapter.clear_active_reverse_ports().await.unwrap();

    assert!(adapter.active_runtime.lock().await.is_none());
}

#[tokio::test]
async fn preparing_apply_persists_both_generations_without_removing_old_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-A".into());
    let listener_id = ListenerId::new();
    let activation = test_activation("profile-new", "203.0.113.20", listener_id, 36_127);
    let old_port = allocated_reverse_ports(&activation.proxy_routes)[&listener_id.to_string()];
    let (old_reverse, old_runtime) =
        seed_active_runtime(&adapter, "DEVICE-A", vec![old_port]).await;

    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation, AndroidRuntimeOwnerSource::Apply)
        .await
        .unwrap();

    let staged = adapter.active_reverse.lock().await.clone().unwrap();
    let mut expected_ports = vec![old_port, prepared.reverse.as_ref().unwrap().ports[0]];
    expected_ports.sort_unstable();
    assert_eq!(staged.epoch, prepared.owner.epoch);
    assert_eq!(staged.ports, expected_ports);
    let persisted = adapter
        .runtime_store
        .load_android_runtime_owner()
        .unwrap()
        .unwrap();
    assert_eq!(persisted.owner.epoch, prepared.owner.epoch);
    assert_eq!(persisted.reverse_ports, expected_ports);
    assert_eq!(
        *adapter.active_runtime.lock().await,
        Some(old_runtime.clone())
    );
    let remove_old = vec![
        "-s".to_owned(),
        "DEVICE-A".to_owned(),
        "reverse".to_owned(),
        "--remove".to_owned(),
        format!("tcp:{old_port}"),
    ];
    assert!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| args != &remove_old)
    );

    let error = adapter
        .finish_prepared_network_update::<()>(
            prepared,
            Err(AppError::new(
                "ANDROID_CONTROL_SOCKET_FAILED",
                "apply failed",
            )),
        )
        .await
        .expect_err("control apply failure must remain observable");
    assert_eq!(error.view_model.code, "ANDROID_CONTROL_SOCKET_FAILED");
    assert_eq!(*adapter.active_reverse.lock().await, Some(old_reverse));
    assert_eq!(*adapter.active_runtime.lock().await, Some(old_runtime));
    assert!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| args != &remove_old),
        "rolling back a failed apply must leave the active reverse endpoint intact",
    );
}

#[tokio::test]
async fn accepted_but_unconfirmed_apply_retains_both_reverse_generations() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-A".into());
    let listener_id = ListenerId::new();
    let activation = test_activation("profile-new", "203.0.113.20", listener_id, 36_127);
    let old_port = allocated_reverse_ports(&activation.proxy_routes)[&listener_id.to_string()];
    let (_, old_runtime) = seed_active_runtime(&adapter, "DEVICE-A", vec![old_port]).await;
    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation, AndroidRuntimeOwnerSource::Apply)
        .await
        .unwrap();
    let staged_port = prepared.reverse.as_ref().unwrap().ports[0];

    let error = adapter
        .retain_uncertain_network_update::<()>(
            prepared,
            AppError::new(
                "ANDROID_NETWORK_START_CONFIRMATION_TIMEOUT",
                "status unknown",
            ),
        )
        .await
        .expect_err("uncertain activation must remain observable");

    assert_eq!(
        error.view_model.code,
        "ANDROID_NETWORK_START_CONFIRMATION_TIMEOUT"
    );
    let active = adapter.active_reverse.lock().await.clone().unwrap();
    let mut expected_ports = vec![old_port, staged_port];
    expected_ports.sort_unstable();
    assert_eq!(active.ports, expected_ports);
    assert_ne!(
        *adapter.active_runtime.lock().await,
        Some(old_runtime),
        "status reconciliation must compare against the accepted new runtime",
    );
    assert!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| !args.contains(&"--remove".to_owned())),
        "a possibly active device endpoint must not be removed",
    );
}

#[tokio::test]
async fn successful_apply_publishes_staged_mapping_before_retiring_old_mapping() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-A".into());
    let listener_id = ListenerId::new();
    let activation = test_activation("profile-new", "203.0.113.20", listener_id, 36_127);
    let old_port = allocated_reverse_ports(&activation.proxy_routes)[&listener_id.to_string()];
    seed_active_runtime(&adapter, "DEVICE-A", vec![old_port]).await;
    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation, AndroidRuntimeOwnerSource::Apply)
        .await
        .unwrap();
    let staged_port = prepared.reverse.as_ref().unwrap().ports[0];

    assert_eq!(
        adapter
            .finish_prepared_network_update(prepared, Ok("applied"))
            .await
            .unwrap(),
        "applied",
    );

    let active = adapter.active_reverse.lock().await.clone().unwrap();
    assert_eq!(active.profile_id, "profile-new");
    assert_eq!(active.ports, vec![staged_port]);
    assert_ne!(staged_port, old_port);
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[1][2], "reverse");
    assert_eq!(calls[2][3], "--remove");
    assert_eq!(calls[2][4], format!("tcp:{old_port}"));
}

#[tokio::test]
async fn failed_reverse_cleanup_retains_only_failed_ports_for_retry() {
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
                stderr: "cannot remove tcp:31628".into(),
            },
            AdbOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            },
        ])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
        epoch: uuid::Uuid::new_v4(),
        serial: "DEVICE-A".into(),
        profile_id: "profile-a".into(),
        ports: vec![31_627, 31_628],
    });

    let error = adapter
        .clear_active_reverse_ports()
        .await
        .expect_err("部分删除失败必须可观察");
    assert_eq!(error.view_model.code, "ANDROID_ADB_REVERSE_CLEANUP_FAILED");
    assert_eq!(
        adapter.active_reverse.lock().await.as_ref().unwrap().ports,
        vec![31_628]
    );

    adapter
        .clear_active_reverse_ports()
        .await
        .expect("重试仅删除仍归属当前运行态的端口");
    assert!(adapter.active_reverse.lock().await.is_none());
    assert_eq!(
        *runner.calls.lock().unwrap(),
        vec![
            vec!["-s", "DEVICE-A", "reverse", "--remove", "tcp:31627"],
            vec!["-s", "DEVICE-A", "reverse", "--remove", "tcp:31628"],
            vec!["-s", "DEVICE-A", "reverse", "--remove", "tcp:31628"],
        ]
    );
}

#[tokio::test]
async fn missing_reverse_listener_is_an_idempotent_cleanup_success() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            AdbOutput {
                success: false,
                stdout: String::new(),
                stderr: "adb.exe: error: listener 'tcp:40163' not found".into(),
            },
            AdbOutput {
                success: true,
                stdout: "List of devices attached\nDEVICE-B device model:A920MAX\n".into(),
                stderr: String::new(),
            },
            AdbOutput {
                success: true,
                stdout: "Android Debug Bridge version 1.0.41".into(),
                stderr: String::new(),
            },
        ])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
        epoch: uuid::Uuid::new_v4(),
        serial: "DEVICE-A".into(),
        profile_id: "profile-a".into(),
        ports: vec![40_163],
    });

    adapter
        .clear_active_reverse_ports()
        .await
        .expect("已经不存在的 reverse 映射应视为清理完成");

    assert!(adapter.active_reverse.lock().await.is_none());
    assert!(adapter.active_runtime.lock().await.is_none());

    let selected = adapter
        .adb_select("DEVICE-B".into())
        .await
        .expect("幂等停止完成后不应再被陈旧映射阻止切换设备");
    assert_eq!(selected.selected_serial.as_deref(), Some("DEVICE-B"));
    assert_eq!(
        *runner.calls.lock().unwrap(),
        vec![
            vec!["-s", "DEVICE-A", "reverse", "--remove", "tcp:40163"],
            vec!["devices", "-l"],
            vec!["version"],
        ]
    );
}

#[tokio::test]
async fn device_selection_can_change_without_transferring_runtime_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            AdbOutput {
                success: true,
                stdout: "List of devices attached\nDEVICE-B device model:A920MAX\n".into(),
                stderr: String::new(),
            },
            AdbOutput {
                success: true,
                stdout: "Android Debug Bridge version 1.0.41".into(),
                stderr: String::new(),
            },
        ])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
        epoch: uuid::Uuid::new_v4(),
        serial: "DEVICE-A".into(),
        profile_id: "profile-a".into(),
        ports: vec![31_627],
    });

    let selected = adapter
        .adb_select("DEVICE-B".into())
        .await
        .expect("选择设备只改变编辑上下文");

    assert_eq!(selected.selected_serial.as_deref(), Some("DEVICE-B"));
    assert_eq!(
        adapter.active_reverse.lock().await.as_ref().unwrap().serial,
        "DEVICE-A"
    );
}
