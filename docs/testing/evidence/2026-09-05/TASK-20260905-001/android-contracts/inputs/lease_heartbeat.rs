use super::*;
use std::{path::Path, sync::Mutex as StdMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Default)]
struct HeartbeatRunner {
    owner_epochs: Arc<StdMutex<Vec<String>>>,
}

#[async_trait]
impl AdbCommandRunner for HeartbeatRunner {
    async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        if args.get(2).map(String::as_str) != Some("forward")
            || args.get(3).map(String::as_str) == Some("--remove")
        {
            return Ok(successful_adb_output());
        }
        let port = args[3]
            .strip_prefix("tcp:")
            .expect("forward port")
            .parse::<u16>()
            .expect("numeric forward port");
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        let owner_epochs = Arc::clone(&self.owner_epochs);
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut prefix = [0_u8; 4];
            stream.read_exact(&mut prefix).await.unwrap();
            let mut payload = vec![0_u8; u32::from_be_bytes(prefix) as usize];
            stream.read_exact(&mut payload).await.unwrap();
            let request: serde_json::Value =
                serde_json::from_slice(&payload).expect("heartbeat request");
            owner_epochs.lock().unwrap().push(
                request["payload"]["owner_epoch"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
            let response = serde_json::json!({
                "version": intercept_proxy_application::ANDROID_CONTROL_PROTOCOL_VERSION,
                "request_id": request["request_id"],
                "ok": true,
                "status": {
                    "serial": "",
                    "state": "running",
                    "verified": true,
                    "transport": "local_abstract_socket",
                    "active_profile_id": "profile-old",
                    "active_profile_fingerprint": "old-profile",
                    "active_route_fingerprint": "old-routes",
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
            stream
                .write_all(&u32::try_from(response.len()).unwrap().to_be_bytes())
                .await
                .unwrap();
            stream.write_all(&response).await.unwrap();
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
async fn same_serial_business_gate_does_not_block_real_heartbeat_path() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(HeartbeatRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    let (_, runtime) = seed_active_runtime(&adapter, "device-a", Vec::new()).await;
    let gate = adapter.device_operations.gate("device-a");
    let _operation = gate.lock().await;

    tokio::time::timeout(
        Duration::from_millis(500),
        adapter.renew_control_lease_target("device-a", runtime.epoch),
    )
    .await
    .expect("heartbeat must bypass the held same-serial business gate")
    .unwrap();

    assert_eq!(
        *runner.owner_epochs.lock().unwrap(),
        vec![runtime.epoch.to_string()]
    );
}

#[tokio::test]
async fn replaced_epoch_is_rejected_before_real_heartbeat_transport() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(HeartbeatRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    let (_, old_runtime) = seed_active_runtime(&adapter, "device-a", Vec::new()).await;
    let mut replacement = old_runtime.clone();
    replacement.epoch = uuid::Uuid::new_v4();
    {
        let mut states = adapter.owner_states.lock().await;
        let state = states.get_mut("device-a").unwrap();
        state.runtime_owner = Some(replacement.owner(
            AndroidRuntimeOwnerMode::AdbReverse,
            AndroidRuntimeOwnerSource::Apply,
            AndroidRuntimeOwnerState::Active,
            AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        ));
        state.active_runtime = Some(replacement);
    }

    adapter
        .renew_control_lease_target("device-a", old_runtime.epoch)
        .await
        .unwrap();

    assert!(runner.owner_epochs.lock().unwrap().is_empty());
}
