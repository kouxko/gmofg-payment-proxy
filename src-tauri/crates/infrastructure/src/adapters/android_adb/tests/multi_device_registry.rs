use super::*;
use intercept_proxy_application::{
    ANDROID_CONTROL_PROTOCOL_VERSION, AndroidCompanionStatus, AndroidControlRequest,
    AndroidControlResponse, AndroidControlTransport, AndroidDeviceTarget, AndroidNetworkState,
    AndroidRuntimeTarget, encode_android_control_frame,
};
use std::io::{Read, Write};

#[derive(Debug)]
struct ReconnectRunner {
    calls: std::sync::Mutex<Vec<Vec<String>>>,
    devices_stdout: String,
    rejected_serial: Option<String>,
}

impl Default for ReconnectRunner {
    fn default() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            devices_stdout: "List of devices attached\nDEVICE-A device\nDEVICE-B device\n".into(),
            rejected_serial: None,
        }
    }
}

#[async_trait]
impl AdbCommandRunner for ReconnectRunner {
    async fn run(&self, _: &std::path::Path, args: &[String]) -> std::io::Result<AdbOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        if args == ["devices", "-l"] {
            return Ok(AdbOutput {
                success: true,
                stdout: self.devices_stdout.clone(),
                stderr: String::new(),
            });
        }
        if args.iter().any(|arg| arg == "--remove") {
            return Ok(successful_adb_output());
        }
        let Some(port) = args
            .iter()
            .find_map(|arg| arg.strip_prefix("tcp:"))
            .and_then(|value| value.parse::<u16>().ok())
        else {
            return Ok(successful_adb_output());
        };
        let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
        let rejected = args
            .get(1)
            .is_some_and(|serial| self.rejected_serial.as_ref() == Some(serial));
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut prefix = [0_u8; 4];
            stream.read_exact(&mut prefix).unwrap();
            let mut payload = vec![0; u32::from_be_bytes(prefix) as usize];
            stream.read_exact(&mut payload).unwrap();
            let request: AndroidControlRequest = serde_json::from_slice(&payload).unwrap();
            let response = AndroidControlResponse {
                version: ANDROID_CONTROL_PROTOCOL_VERSION,
                request_id: request.request_id,
                ok: !rejected,
                status: (!rejected).then_some(AndroidCompanionStatus {
                    serial: String::new(),
                    state: AndroidNetworkState::Running,
                    verified: true,
                    transport: AndroidControlTransport::LocalAbstractSocket,
                    active_profile_id: Some("profile-test".into()),
                    active_profile_fingerprint: None,
                    active_route_fingerprint: None,
                    active_route_count: 0,
                    companion_process_running: Some(true),
                    message: "reconnected".into(),
                    unsupported_fields: Vec::new(),
                    stats: None,
                }),
                error_code: rejected.then(|| "ANDROID_DEVICE_QUERY_FAILED".into()),
                error_message: rejected.then(|| "device query failed".into()),
            };
            stream
                .write_all(&encode_android_control_frame(&response).unwrap())
                .unwrap();
        });
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

#[tokio::test]
async fn same_serial_operations_serialize_while_different_serials_progress() {
    let registry = DeviceOperationGateRegistry::default();
    let gate_a = registry.gate("DEVICE-A");
    let gate_a_again = registry.gate("DEVICE-A");
    let gate_b = registry.gate("DEVICE-B");

    let _held_a = gate_a.lock().await;
    assert!(gate_a_again.try_lock().is_err());
    assert!(gate_b.try_lock().is_ok());
}

#[test]
fn concurrent_control_channels_reserve_distinct_forward_ports() {
    let first = super::super::protocol::reserve_loopback_port().unwrap();
    let second = super::super::protocol::reserve_loopback_port().unwrap();
    assert_ne!(first.port(), second.port());
}

#[tokio::test]
async fn explicit_target_is_not_retargeted_by_selected_device() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());

    let status = adapter
        .network_status(AndroidDeviceTarget {
            serial: "DEVICE-A".into(),
        })
        .await
        .unwrap();

    assert_eq!(status.serial, "DEVICE-A");
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stale_epoch_is_rejected_before_any_adb_side_effect() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    adapter.save_owner(owner.clone()).await.unwrap();

    let current_epoch = owner.epoch;
    let error = adapter
        .network_stop(AndroidRuntimeTarget {
            serial: owner.serial.clone(),
            expected_epoch: uuid::Uuid::new_v4(),
        })
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_RUNTIME_EPOCH_STALE");
    assert_eq!(error.view_model.entity_id.as_deref(), Some("DEVICE-A"));
    assert_eq!(error.view_model.runtime_epoch, Some(current_epoch));
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn missing_owner_is_rejected_with_exact_device_context() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());

    let error = adapter
        .network_stop(AndroidRuntimeTarget {
            serial: "DEVICE-A".into(),
            expected_epoch: uuid::Uuid::new_v4(),
        })
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_RUNTIME_NOT_MANAGED");
    assert_eq!(error.view_model.entity_id.as_deref(), Some("DEVICE-A"));
    assert_eq!(error.view_model.runtime_epoch, None);
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn device_list_marks_only_missing_owner_waiting_reconnect() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: true,
            stdout: "List of devices attached\nDEVICE-B device transport_id:2\n".into(),
            stderr: String::new(),
        }])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner);
    let owner_a = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    let owner_b = runtime_owner("DEVICE-B", AndroidRuntimeOwnerState::Active);
    adapter.save_owner(owner_a.clone()).await.unwrap();
    adapter.save_owner(owner_b.clone()).await.unwrap();

    let devices = adapter.device_list().await.unwrap();

    assert_eq!(devices.len(), 1);
    let current_a = adapter
        .runtime_owner_snapshot_for("DEVICE-A")
        .await
        .unwrap();
    let current_b = adapter
        .runtime_owner_snapshot_for("DEVICE-B")
        .await
        .unwrap();
    assert_eq!(current_a.epoch, owner_a.epoch);
    assert_eq!(current_a.state, AndroidRuntimeOwnerState::WaitingReconnect);
    assert_eq!(
        current_a.transition_reason,
        AndroidRuntimeOwnerTransitionReason::DeviceDisconnected
    );
    assert_eq!(current_b, owner_b);
}

#[tokio::test]
async fn offline_or_unauthorized_label_alone_neither_marks_missing_nor_requests_stop() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: true,
            stdout: "List of devices attached\nDEVICE-A offline\nDEVICE-B unauthorized\n".into(),
            stderr: String::new(),
        }])),
    });
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    let owner_a = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    let owner_b = runtime_owner("DEVICE-B", AndroidRuntimeOwnerState::Active);
    adapter.save_owner(owner_a.clone()).await.unwrap();
    adapter.save_owner(owner_b.clone()).await.unwrap();

    adapter.device_list().await.unwrap();

    assert_eq!(
        adapter.runtime_owner_snapshot_for("DEVICE-A").await,
        Some(owner_a)
    );
    assert_eq!(
        adapter.runtime_owner_snapshot_for("DEVICE-B").await,
        Some(owner_b)
    );
    assert_eq!(
        *runner.calls.lock().unwrap(),
        vec![vec![String::from("devices"), String::from("-l")]],
        "设备状态标签只能更新设备清单；不得直接发送 stop/force-stop"
    );
}

#[tokio::test]
async fn device_list_restores_online_waiting_owner_without_changing_other_owner() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(ReconnectRunner::default()));
    let owner_a = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::WaitingReconnect);
    let owner_b = runtime_owner("DEVICE-B", AndroidRuntimeOwnerState::Active);
    adapter.save_owner(owner_a.clone()).await.unwrap();
    adapter.save_owner(owner_b.clone()).await.unwrap();

    adapter.device_list().await.unwrap();

    let current_a = adapter
        .runtime_owner_snapshot_for("DEVICE-A")
        .await
        .unwrap();
    let current_b = adapter
        .runtime_owner_snapshot_for("DEVICE-B")
        .await
        .unwrap();
    assert_eq!(current_a.epoch, owner_a.epoch);
    assert_eq!(current_a.state, AndroidRuntimeOwnerState::Active);
    assert_eq!(
        current_a.transition_reason,
        AndroidRuntimeOwnerTransitionReason::DeviceReconnected
    );
    assert_eq!(current_b, owner_b);
}

#[tokio::test]
async fn reconnect_failure_keeps_device_context_and_does_not_skip_later_owner() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            AdbOutput {
                success: true,
                stdout: "List of devices attached\nDEVICE-A device\n".into(),
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
    let owner_a = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::WaitingReconnect);
    let owner_b = runtime_owner("DEVICE-B", AndroidRuntimeOwnerState::Active);
    adapter.save_owner(owner_a.clone()).await.unwrap();
    adapter.save_owner(owner_b.clone()).await.unwrap();

    let error = adapter.device_list().await.unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_ADB_COMMAND_FAILED");
    assert!(error.view_model.message.contains("permission denied"));
    assert_eq!(error.view_model.entity_id.as_deref(), Some("DEVICE-A"));
    assert_eq!(error.view_model.runtime_epoch, Some(owner_a.epoch));
    let current_b = adapter
        .runtime_owner_snapshot_for("DEVICE-B")
        .await
        .unwrap();
    assert_eq!(current_b.epoch, owner_b.epoch);
    assert_eq!(current_b.state, AndroidRuntimeOwnerState::WaitingReconnect);
}

#[tokio::test]
async fn runtime_owners_are_sorted_by_serial() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    adapter
        .save_owner(runtime_owner("DEVICE-B", AndroidRuntimeOwnerState::Active))
        .await
        .unwrap();
    adapter
        .save_owner(runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active))
        .await
        .unwrap();

    let serials = adapter
        .runtime_owners()
        .await
        .unwrap()
        .into_iter()
        .map(|owner| owner.serial)
        .collect::<Vec<_>>();
    assert_eq!(serials, vec!["DEVICE-A", "DEVICE-B"]);
}
