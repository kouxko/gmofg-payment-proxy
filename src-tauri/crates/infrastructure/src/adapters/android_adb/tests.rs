use super::*;
use intercept_proxy_application::{
    AndroidNetworkProfile, AndroidProxyRouteActivation, AndroidTargetApplication,
    WeakNetworkProfile,
};
use intercept_proxy_domain::ListenerId;
use std::path::Path;

#[path = "tests/fixtures.rs"]
mod fixtures;
use fixtures::*;

#[path = "tests/forward_control.rs"]
mod forward_control;
#[path = "tests/owner_crash_safety.rs"]
mod owner_crash_safety;
#[path = "tests/owner_lifecycle.rs"]
mod owner_lifecycle;
#[path = "tests/runtime_endpoints.rs"]
mod runtime_endpoints;

#[test]
fn canonical_fingerprint_matches_android_for_cidr_and_url() {
    let value = serde_json::json!({
        "destination_targets": [{
            "address": "10.0.0.0/8",
            "ports": [16_127]
        }],
        "server_url": "https://example.test:16127/path"
    });

    assert_eq!(
        canonical_json(&value),
        r#"{"destination_targets":[{"address":"10.0.0.0/8","ports":[16127]}],"server_url":"https://example.test:16127/path"}"#
    );
    assert_eq!(
        sha256_json(&value).unwrap(),
        "1b9889227509e4d1dca893ffc0d023e82e96258b84c34f03f4d550361a47db1a"
    );
}

#[test]
fn parses_devices_and_packages_with_shared_uid_inputs() {
    let devices = parse_devices(
        "List of devices attached\nSER123 device product:a920 model:A920MAX device:a920 transport_id:7\nOFF offline\n",
        Some("SER123"),
    );
    assert_eq!(devices.len(), 2);
    assert!(devices[0].selected);
    assert_eq!(devices[0].model.as_deref(), Some("A920MAX"));

    let packages =
        parse_packages("package:com.example.one uid:10123\npackage:com.example.two uid:10123\n");
    assert_eq!(packages[0].uid, 10_123);
}

#[test]
fn reverse_runtime_mapping_accepts_adb_serial_prefix() {
    let routes = vec![AndroidProxyRouteActivation {
        listener_id: "listener-a".into(),
        listener_name: "Listener A".into(),
        original_destination: "203.0.113.10".into(),
        original_ports: vec![16_127],
        desktop_listener_bind_address: "0.0.0.0".into(),
        desktop_listener_port: 26_127,
        allowed_client_cidrs: Vec::new(),
    }];
    let device_port = allocated_reverse_ports(&routes)["listener-a"];

    assert!(reverse_mapping_present(
        &format!("SER123 tcp:{device_port} tcp:26127\n"),
        device_port,
        26_127,
    ));
    assert!(!reverse_mapping_present(
        &format!("SER123 tcp:{device_port} tcp:16627\n"),
        device_port,
        26_127,
    ));
}

#[test]
fn packaged_apk_path_is_owned_by_adapter_not_command_input() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    assert_eq!(
        adapter.companion_apk.as_deref(),
        Some(temp.path().join("android-companion.apk").as_path())
    );
}

#[test]
fn packaged_apk_candidates_include_the_tauri_macos_resource_layout() {
    let executable = Path::new("/Applications/Intercept Proxy.app/Contents/MacOS/intercept-proxy");
    let candidates = bundled_companion_apk_candidates(executable);

    assert!(candidates.contains(&PathBuf::from(
            "/Applications/Intercept Proxy.app/Contents/MacOS/../Resources/resources/android-companion.apk",
        )));
}

#[test]
fn packaged_apk_candidates_include_windows_installed_and_portable_layout() {
    // Windows 接受正斜杠路径；这样同一测试也能在 macOS/Linux CI 验证目录布局。
    let executable = Path::new("C:/Program Files/Intercept Proxy/Intercept-Proxy.exe");
    let candidates = bundled_companion_apk_candidates(executable);

    assert!(candidates.contains(&PathBuf::from(
        "C:/Program Files/Intercept Proxy/resources/android-companion.apk"
    )));
}

#[tokio::test]
async fn package_inventory_does_not_read_apk_signatures() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: true,
            stdout: "package:com.example.client uid:10001\n".into(),
            stderr: String::new(),
        }])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("2740072778".into());

    let packages = adapter.package_list().await.unwrap();

    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].package_name, "com.example.client");
    assert_eq!(
        *runner.calls.lock().unwrap(),
        vec![vec![
            "-s",
            "2740072778",
            "shell",
            "pm",
            "list",
            "packages",
            "-U",
        ]]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn command_timeout_terminates_the_spawned_process() {
    let temp = tempfile::tempdir().unwrap();
    let pid_path = temp.path().join("adb-child.pid");
    let script = format!("echo $$ > '{}'; exec sleep 30", pid_path.display());
    let mut adapter =
        AndroidAdbAdapter::with_runner(temp.path(), Arc::new(command::SystemAdbCommandRunner));
    adapter.adb_path = Some(PathBuf::from("/bin/sh"));

    let error = adapter
        .run(vec!["-c".into(), script], Duration::from_millis(100))
        .await
        .unwrap_err();
    assert_eq!(error.view_model.code, "ANDROID_ADB_TIMEOUT");
    let pid = std::fs::read_to_string(&pid_path).unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    let still_running = std::process::Command::new("kill")
        .args(["-0", pid.trim()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    assert!(!still_running, "timed-out adb child {pid} is still running");
}

#[path = "tests/reverse.rs"]
mod reverse;
