#[tokio::test]
async fn usb_runtime_creates_reverse_and_keeps_endpoint_out_of_profile() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("SER123".into());
    let listener_id = ListenerId::new();
    let profile = AndroidNetworkProfile {
        id: "route-profile".into(),
        name: "路由".into(),
        target_applications: vec![AndroidTargetApplication {
            package_name: "com.example.target".into(),
            uid: 10_001,
            display_name: None,
        }],
        destination_targets: Vec::new(),
        proxy_routes: vec![intercept_proxy_domain::AndroidProxyRoute {
            destination: "203.0.113.10".into(),
            ports: vec![16_127],
            listener_id,
        }],
        confirmed_shared_uids: std::collections::BTreeSet::default(),
        auto_resume_after_reboot: false,
        weak_network: WeakNetworkProfile::default(),
    };
    let activation = AndroidNetworkActivation {
        profile: profile.clone(),
        proxy_routes: vec![AndroidProxyRouteActivation {
            listener_id: listener_id.to_string(),
            original_destination: "203.0.113.10".into(),
            original_ports: vec![16_127],
            desktop_listener_port: 26_127,
        }],
    };

    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation)
        .await
        .unwrap();
    let runtime = prepared.payload;
    assert_eq!(runtime["route_count"], 1);
    assert_eq!(
        runtime["route_source"][0]["listener_id"],
        listener_id.to_string()
    );
    assert!(runtime["profile_fingerprint"].as_str().is_some());
    assert!(runtime["route_fingerprint"].as_str().is_some());
    assert_eq!(
        runtime["route_fingerprint"],
        sha256_json(&runtime["routes"]).unwrap()
    );
    assert_ne!(
        runtime["route_fingerprint"],
        sha256_json(&runtime["route_source"]).unwrap()
    );
    let route = &runtime["routes"][0];
    assert_eq!(route["proxy_host"], "127.0.0.1");
    assert_eq!(route["original_destination"], "203.0.113.10");
    assert!(route["proxy_port"].as_u64().is_some());
    assert!(
        !serde_json::to_value(profile)
            .unwrap()
            .to_string()
            .contains("proxy_host")
    );
    let calls = runner.calls.lock().unwrap();
    assert!(calls.iter().any(|args| {
        args.windows(2)
            .any(|pair| pair[0] == "reverse" && pair[1].starts_with("tcp:"))
            && args.last() == Some(&"tcp:26127".to_owned())
    }));
}

#[tokio::test]
async fn normalized_route_fingerprint_changes_when_runtime_endpoint_changes() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner);
    *adapter.selected_serial.write().unwrap() = Some("SER123".into());
    let listener_id = ListenerId::new();
    let activation = AndroidNetworkActivation {
        profile: AndroidNetworkProfile {
            id: "endpoint-fingerprint".into(),
            name: "endpoint-fingerprint".into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            proxy_routes: vec![intercept_proxy_domain::AndroidProxyRoute {
                destination: "203.0.113.10".into(),
                ports: vec![16_127],
                listener_id,
            }],
            confirmed_shared_uids: std::collections::BTreeSet::default(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        },
        proxy_routes: vec![AndroidProxyRouteActivation {
            listener_id: listener_id.to_string(),
            original_destination: "203.0.113.10".into(),
            original_ports: vec![16_127],
            desktop_listener_port: 26_127,
        }],
    };

    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation)
        .await
        .unwrap();
    let runtime = prepared.payload;
    let declared = runtime["route_fingerprint"].as_str().unwrap();
    let mut wrong_routes = runtime["routes"].clone();
    wrong_routes[0]["proxy_port"] = json!(49_999);

    assert_ne!(declared, sha256_json(&wrong_routes).unwrap());
}

#[tokio::test]
async fn reverse_cleanup_uses_the_device_that_created_the_mapping() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());
    *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
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
async fn preparing_apply_keeps_active_reverse_until_control_succeeds() {
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
        .prepare_usb_proxy_runtime(&activation)
        .await
        .unwrap();

    assert_eq!(
        *adapter.active_reverse.lock().await,
        Some(old_reverse.clone()),
        "preparing a replacement must not publish or remove it before Android accepts apply",
    );
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
        .prepare_usb_proxy_runtime(&activation)
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
        .prepare_usb_proxy_runtime(&activation)
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
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0][2], "reverse");
    assert_eq!(calls[1][3], "--remove");
    assert_eq!(calls[1][4], format!("tcp:{old_port}"));
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
async fn device_switch_is_rejected_while_reverse_mapping_is_active() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.active_reverse.lock().await = Some(ActiveReverseOwnership {
        serial: "DEVICE-A".into(),
        profile_id: "profile-a".into(),
        ports: vec![31_627],
    });

    let error = adapter
        .adb_select("DEVICE-B".into())
        .await
        .expect_err("活动映射期间不能切换设备");

    assert_eq!(error.view_model.code, "ANDROID_DEVICE_SWITCH_REQUIRES_STOP");
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn destination_resolution_failure_does_not_create_reverse_mapping() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("SER123".into());
    let (old_reverse, old_runtime) = seed_active_runtime(&adapter, "SER123", vec![31_627]).await;
    let activation = test_activation(
        "invalid-dns",
        "invalid destination",
        ListenerId::new(),
        8_443,
    );

    let error = adapter
        .prepare_usb_proxy_runtime(&activation)
        .await
        .expect_err("非法域名必须解析失败");

    assert_eq!(
        error.view_model.code,
        "ANDROID_PROXY_DESTINATION_RESOLVE_FAILED"
    );
    assert!(runner.calls.lock().unwrap().is_empty());
    assert_eq!(*adapter.active_reverse.lock().await, Some(old_reverse));
    assert_eq!(*adapter.active_runtime.lock().await, Some(old_runtime));
}

#[tokio::test]
async fn reverse_creation_failure_keeps_active_mapping_and_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: false,
            stdout: String::new(),
            stderr: "cannot create staged reverse".into(),
        }])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("SER123".into());
    let (old_reverse, old_runtime) = seed_active_runtime(&adapter, "SER123", vec![31_627]).await;
    let activation = test_activation("profile-new", "203.0.113.10", ListenerId::new(), 8_443);

    let error = adapter
        .prepare_usb_proxy_runtime(&activation)
        .await
        .expect_err("staged reverse creation must fail");

    assert_eq!(error.view_model.code, "ANDROID_ADB_REVERSE_CREATE_FAILED");
    assert_eq!(*adapter.active_reverse.lock().await, Some(old_reverse));
    assert_eq!(*adapter.active_runtime.lock().await, Some(old_runtime));
    assert_eq!(runner.calls.lock().unwrap().len(), 1);
    assert!(!runner.calls.lock().unwrap()[0].contains(&"--remove".to_owned()));
}
use super::*;
