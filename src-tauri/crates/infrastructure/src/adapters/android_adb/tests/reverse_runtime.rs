use super::super::*;

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
            desktop_listener_bind_address: "0.0.0.0".into(),
            desktop_listener_port: 26_127,
            allowed_client_cidrs: Vec::new(),
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
            desktop_listener_bind_address: "0.0.0.0".into(),
            desktop_listener_port: 26_127,
            allowed_client_cidrs: Vec::new(),
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
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            AdbOutput {
                success: true,
                stdout: String::new(),
                stderr: String::new(),
            },
            AdbOutput {
                success: false,
                stdout: String::new(),
                stderr: "cannot create staged reverse".into(),
            },
        ])),
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
    assert_eq!(runner.calls.lock().unwrap().len(), 2);
    assert!(!runner.calls.lock().unwrap()[1].contains(&"--remove".to_owned()));
}
