use super::*;
use std::{collections::HashMap, path::Path, sync::Mutex as StdMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Default)]
struct ForwardLifecycleRunner {
    calls: StdMutex<Vec<Vec<String>>>,
    servers: StdMutex<HashMap<u16, tokio::task::JoinHandle<()>>>,
}

#[async_trait]
impl AdbCommandRunner for ForwardLifecycleRunner {
    async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        if args.get(2).map(String::as_str) != Some("forward") {
            return Ok(successful_adb_output());
        }
        if args.get(3).map(String::as_str) == Some("--remove") {
            let port = forwarded_port(&args[4]);
            if let Some(server) = self.servers.lock().unwrap().remove(&port) {
                server.abort();
            }
            return Ok(successful_adb_output());
        }

        let serial = args[1].clone();
        let port = forwarded_port(&args[3]);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut prefix = [0_u8; 4];
            stream.read_exact(&mut prefix).await.unwrap();
            let mut payload = vec![0_u8; u32::from_be_bytes(prefix) as usize];
            stream.read_exact(&mut payload).await.unwrap();
            if serial == "device-a" {
                std::future::pending::<()>().await;
            }
            let request: serde_json::Value = serde_json::from_slice(&payload).unwrap();
            let response = serde_json::json!({
                "version": intercept_proxy_application::ANDROID_CONTROL_PROTOCOL_VERSION,
                "request_id": request["request_id"],
                "ok": true,
                "status": {
                    "serial": "",
                    "state": "running",
                    "verified": true,
                    "transport": "local_abstract_socket",
                    "active_profile_id": "profile",
                    "active_profile_fingerprint": "profile-fingerprint",
                    "active_route_fingerprint": "route-fingerprint",
                    "active_route_count": 1,
                    "companion_process_running": true,
                    "message": "running",
                    "unsupported_fields": [],
                    "stats": null
                },
                "error_code": null,
                "error_message": null
            });
            let response = serde_json::to_vec(&response).unwrap();
            let response_length = u32::try_from(response.len()).unwrap();
            stream
                .write_all(&response_length.to_be_bytes())
                .await
                .unwrap();
            stream.write_all(&response).await.unwrap();
        });
        self.servers.lock().unwrap().insert(port, server);
        Ok(successful_adb_output())
    }
}

fn successful_adb_output() -> AdbOutput {
    AdbOutput {
        success: true,
        stdout: String::new(),
        stderr: String::new(),
    }
}

fn forwarded_port(argument: &str) -> u16 {
    argument.strip_prefix("tcp:").unwrap().parse().unwrap()
}

fn serial_forward_calls(calls: &[Vec<String>], serial: &str) -> Vec<Vec<String>> {
    calls
        .iter()
        .filter(|call| call.get(1).map(String::as_str) == Some(serial))
        .cloned()
        .collect()
}

#[tokio::test]
async fn cancelled_stalled_response_removes_owned_forward_without_blocking_other_serial() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(ForwardLifecycleRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());

    let stalled = tokio::time::timeout(
        Duration::from_millis(50),
        adapter.protocol_request(
            "device-a",
            "heartbeat",
            serde_json::json!({"owner_epoch": "epoch-a"}),
        ),
    );
    let healthy = adapter.protocol_request(
        "device-b",
        "heartbeat",
        serde_json::json!({"owner_epoch": "epoch-b"}),
    );
    let (stalled, healthy) = tokio::join!(stalled, healthy);

    assert!(
        stalled.is_err(),
        "device A response must hit the outer deadline"
    );
    assert!(healthy.is_ok(), "device B must renew while device A stalls");
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let cleanup_finished = {
                let calls = runner.calls.lock().unwrap();
                serial_forward_calls(&calls, "device-a").len() == 2
            };
            if cleanup_finished {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled request must schedule exact forward cleanup");

    let calls = runner.calls.lock().unwrap();
    for serial in ["device-a", "device-b"] {
        let serial_calls = serial_forward_calls(&calls, serial);
        assert_eq!(serial_calls.len(), 2);
        assert_eq!(serial_calls[0][2], "forward");
        assert_eq!(serial_calls[1][..4], ["-s", serial, "forward", "--remove"]);
        assert_eq!(serial_calls[0][3], serial_calls[1][4]);
    }
}

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

#[test]
fn pending_activation_renews_enabled_lease_once_per_second() {
    let renewal_attempts = (0..20)
        .filter(|attempt| should_renew_activation_lease(true, *attempt))
        .collect::<Vec<_>>();

    assert_eq!(renewal_attempts, vec![0, 4, 8, 12, 16]);
    assert!(!(0..20).any(|attempt| should_renew_activation_lease(false, attempt)));
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
