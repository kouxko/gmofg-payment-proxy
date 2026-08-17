use super::*;

#[tokio::test]
async fn force_stop_companion_closes_tun_without_control_socket() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("SER123".into());

    let status = adapter.force_stop_companion("SER123").await.unwrap();

    assert_eq!(status.state, AndroidNetworkState::Stopped);
    assert!(status.verified);
    assert_eq!(status.transport, AndroidControlTransport::AdbForceStop);
    assert_eq!(
        *runner.calls.lock().unwrap(),
        vec![vec![
            "-s",
            "SER123",
            "shell",
            "am",
            "force-stop",
            ANDROID_COMPANION_PACKAGE,
        ]]
    );
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

    adapter.wake_control_server("2740072778").await.unwrap();

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
fn real_forward_cleanup_failure_is_reported_after_successful_control_request() {
    let cleanup = Err(AppError::new(
        "ANDROID_ADB_COMMAND_FAILED",
        "cannot remove forward",
    ));
    let error = reconcile_forward_cleanup(Ok("response"), cleanup).unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_ADB_FORWARD_CLEANUP_FAILED");
}

#[test]
fn missing_forward_listener_is_idempotent_after_successful_control_request() {
    let cleanup = Err(AppError::new(
        "ANDROID_ADB_COMMAND_FAILED",
        "adb.exe: error: listener 'tcp:40163' not found",
    ));

    let response = reconcile_forward_cleanup(Ok("response"), cleanup).unwrap();

    assert_eq!(response, "response");
}

#[test]
fn missing_forward_listener_does_not_obscure_the_primary_error() {
    let operation = Err::<(), _>(AppError::new(
        "ANDROID_CONTROL_SOCKET_FAILED",
        "control request failed",
    ));
    let cleanup = Err(AppError::new(
        "ANDROID_ADB_COMMAND_FAILED",
        "adb 命令失败：adb.exe: error: listener 'tcp:40163' not found",
    ));

    let error = reconcile_forward_cleanup(operation, cleanup).unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_CONTROL_SOCKET_FAILED");
    assert_eq!(error.view_model.message, "control request failed");
}
