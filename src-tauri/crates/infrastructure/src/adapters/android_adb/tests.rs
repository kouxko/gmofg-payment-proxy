use super::*;
use intercept_proxy_application::{
    AndroidNetworkProfile, AndroidProxyRouteActivation, AndroidTargetApplication,
    WeakNetworkProfile,
};
use intercept_proxy_domain::ListenerId;
use std::path::Path;

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
        original_destination: "203.0.113.10".into(),
        original_ports: vec![16_127],
        desktop_listener_port: 26_127,
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

#[derive(Debug)]
struct FakeRunner;

#[async_trait]
impl AdbCommandRunner for FakeRunner {
    async fn run(&self, _: &Path, _: &[String]) -> std::io::Result<AdbOutput> {
        Ok(AdbOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

#[derive(Debug, Default)]
struct RecordingRunner {
    calls: std::sync::Mutex<Vec<Vec<String>>>,
}

#[async_trait]
impl AdbCommandRunner for RecordingRunner {
    async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(AdbOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

#[derive(Debug)]
struct SequenceRunner {
    calls: std::sync::Mutex<Vec<Vec<String>>>,
    outputs: std::sync::Mutex<std::collections::VecDeque<AdbOutput>>,
}

#[async_trait]
impl AdbCommandRunner for SequenceRunner {
    async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(self
            .outputs
            .lock()
            .unwrap()
            .pop_front()
            .expect("测试必须为每次 adb 调用提供结果"))
    }
}

fn test_activation(
    profile_id: &str,
    destination: &str,
    listener_id: ListenerId,
    desktop_listener_port: u16,
) -> AndroidNetworkActivation {
    AndroidNetworkActivation {
        profile: AndroidNetworkProfile {
            id: profile_id.into(),
            name: profile_id.into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            proxy_routes: vec![intercept_proxy_domain::AndroidProxyRoute {
                destination: destination.into(),
                ports: vec![443],
                listener_id,
            }],
            confirmed_shared_uids: std::collections::BTreeSet::default(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        },
        proxy_routes: vec![AndroidProxyRouteActivation {
            listener_id: listener_id.to_string(),
            original_destination: destination.into(),
            original_ports: vec![443],
            desktop_listener_port,
        }],
    }
}

async fn seed_active_runtime(
    adapter: &AndroidAdbAdapter,
    serial: &str,
    ports: Vec<u16>,
) -> (ActiveReverseOwnership, ActiveRuntimeFacts) {
    let reverse = ActiveReverseOwnership {
        serial: serial.into(),
        profile_id: "profile-old".into(),
        ports,
    };
    let runtime = ActiveRuntimeFacts {
        serial: serial.into(),
        profile_id: "profile-old".into(),
        profile_fingerprint: "old-profile".into(),
        route_fingerprint: "old-routes".into(),
        route_count: 1,
        listener_ports: BTreeMap::new(),
    };
    *adapter.active_reverse.lock().await = Some(reverse.clone());
    *adapter.active_runtime.lock().await = Some(runtime.clone());
    (reverse, runtime)
}

fn activation_status(
    state: AndroidNetworkState,
    verified: bool,
    profile_fingerprint: Option<&str>,
    route_fingerprint: Option<&str>,
) -> AndroidNetworkStatusViewModel {
    AndroidNetworkStatusViewModel {
        serial: "2740072778".into(),
        state,
        state_text: String::new(),
        ui_tone: intercept_proxy_application::UiTone::Warning,
        verified,
        transport: AndroidControlTransport::LocalAbstractSocket,
        active_profile_id: Some("profile-new".into()),
        active_profile_fingerprint: profile_fingerprint.map(str::to_owned),
        active_route_fingerprint: route_fingerprint.map(str::to_owned),
        active_route_count: 2,
        companion_process_running: Some(true),
        message: "test status".into(),
        unsupported_fields: Vec::new(),
        stats: None,
    }
}

fn activation_runtime() -> ActiveRuntimeFacts {
    ActiveRuntimeFacts {
        serial: "2740072778".into(),
        profile_id: "profile-new".into(),
        profile_fingerprint: "profile-fingerprint".into(),
        route_fingerprint: "route-fingerprint".into(),
        route_count: 2,
        listener_ports: BTreeMap::new(),
    }
}

#[test]
fn accepted_start_request_is_not_final_activation_success() {
    let status = activation_status(
        AndroidNetworkState::StartRequested,
        true,
        Some("profile-fingerprint"),
        Some("route-fingerprint"),
    );

    assert_eq!(
        classify_activation_status(&activation_runtime(), &status),
        ActivationObservation::Pending
    );
}

#[test]
fn only_verified_running_state_with_current_fingerprints_is_confirmed() {
    let matching = activation_status(
        AndroidNetworkState::Running,
        true,
        Some("profile-fingerprint"),
        Some("route-fingerprint"),
    );
    let stale = activation_status(
        AndroidNetworkState::Running,
        true,
        Some("old-profile-fingerprint"),
        Some("route-fingerprint"),
    );

    assert_eq!(
        classify_activation_status(&activation_runtime(), &matching),
        ActivationObservation::Confirmed
    );
    assert_eq!(
        classify_activation_status(&activation_runtime(), &stale),
        ActivationObservation::Pending
    );
}

#[test]
fn companion_fault_is_a_terminal_activation_failure() {
    let status = activation_status(
        AndroidNetworkState::Faulted,
        false,
        Some("profile-fingerprint"),
        Some("route-fingerprint"),
    );

    assert_eq!(
        classify_activation_status(&activation_runtime(), &status),
        ActivationObservation::Faulted
    );
}

#[tokio::test]
async fn wake_control_server_does_not_wait_for_activity_completion() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("2740072778".into());

    adapter.wake_control_server().await.unwrap();

    assert_eq!(
        *runner.calls.lock().unwrap(),
        vec![vec![
            "-s",
            "2740072778",
            "shell",
            "am",
            "start",
            "-n",
            "com.interceptproxy.vpn/.AdbControlActivity",
            "--es",
            "command",
            "wake_control_server",
        ]]
    );
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

#[tokio::test]
async fn stale_multi_device_forward_fails_without_mutating_other_transports() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: false,
            stdout: String::new(),
            stderr: "adb: error: more than one device/emulator".into(),
        }])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());

    let error = adapter
        .run_forward_for_serial(
            "2740072778",
            &["forward", "tcp:47123", "localabstract:intercept_proxy_vpn"],
        )
        .await
        .expect_err("陈旧 transport 必须由显式设备刷新恢复");

    assert_eq!(
        error.view_model.code,
        "ANDROID_ADB_SELECTED_TRANSPORT_STALE"
    );
    assert_eq!(
        *runner.calls.lock().unwrap(),
        vec![vec![
            "-s",
            "2740072778",
            "forward",
            "tcp:47123",
            "localabstract:intercept_proxy_vpn",
        ]]
    );
}

#[test]
fn forward_cleanup_failure_is_observable_after_successful_request() {
    let cleanup = Err(AppError::new(
        "ANDROID_ADB_COMMAND_FAILED",
        "cannot remove forward",
    ));
    let error = reconcile_forward_cleanup(Ok("response"), cleanup).unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_ADB_FORWARD_CLEANUP_FAILED");
    assert!(error.view_model.message.contains("cannot remove forward"));
}

#[cfg(unix)]
#[tokio::test]
async fn command_timeout_terminates_the_spawned_process() {
    let temp = tempfile::tempdir().unwrap();
    let pid_path = temp.path().join("adb-child.pid");
    let script = format!("echo $$ > '{}'; exec sleep 30", pid_path.display());
    let mut adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(SystemAdbCommandRunner));
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
