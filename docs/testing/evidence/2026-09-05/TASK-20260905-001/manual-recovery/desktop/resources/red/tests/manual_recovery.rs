use super::*;
use async_trait::async_trait;
use intercept_proxy_application::{
    AndroidControlPort, AndroidNetworkActivation, AndroidNetworkState,
    AndroidRuntimeEndpointHealth, AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource,
    AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason, AndroidRuntimeTarget,
};
use serde_json::{Value, json};
use std::{collections::HashMap, net::Ipv4Addr, sync::Mutex as StdMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug)]
struct DesktopLan;

impl DeviceLanAddressProvider for DesktopLan {
    fn local_ipv4_for(&self, _: Ipv4Addr) -> Option<Ipv4Addr> {
        Some(Ipv4Addr::new(192, 168, 1, 20))
    }
}

#[derive(Debug, Default)]
struct RecoveryRunner {
    requests: Arc<StdMutex<Vec<Value>>>,
    servers: StdMutex<HashMap<u16, tokio::task::JoinHandle<()>>>,
}

#[async_trait]
impl AdbCommandRunner for RecoveryRunner {
    async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        let output = AdbOutput {
            success: true,
            stdout: "5: wlan0 inet 192.168.1.99/24 scope global wlan0\n".into(),
            stderr: String::new(),
        };
        if args.get(2).map(String::as_str) != Some("forward") {
            return Ok(output);
        }
        let remove = args.get(3).map(String::as_str) == Some("--remove");
        let port = args[if remove { 4 } else { 3 }]
            .strip_prefix("tcp:")
            .unwrap()
            .parse::<u16>()
            .unwrap();
        if remove {
            if let Some(server) = self.servers.lock().unwrap().remove(&port) {
                server.abort();
            }
            return Ok(output);
        }
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        let requests = Arc::clone(&self.requests);
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut prefix = [0; 4];
            stream.read_exact(&mut prefix).await.unwrap();
            let mut bytes = vec![0; u32::from_be_bytes(prefix) as usize];
            stream.read_exact(&mut bytes).await.unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            eprintln!("CONTROL_REQUEST {}", String::from_utf8(bytes).unwrap());
            requests.lock().unwrap().push(request.clone());
            let is_apply = request["operation"] == "apply";
            let payload = &request["payload"];
            let lease_valid = payload["control_lease"]["owner_epoch"].as_str().is_some()
                && payload["control_lease"]["timeout_millis"] == 5_000;
            let accepted = !is_apply || lease_valid;
            let runtime = &payload["proxy_runtime"];
            let response = json!({
                "version": intercept_proxy_application::ANDROID_CONTROL_PROTOCOL_VERSION,
                "request_id": request["request_id"],
                "ok": accepted,
                "status": accepted.then(|| json!({
                    "serial": "", "state": if is_apply { "running" } else { "stopped" },
                    "verified": true, "transport": "local_abstract_socket",
                    "active_profile_id": payload["profile"]["id"],
                    "active_profile_fingerprint": runtime["profile_fingerprint"],
                    "active_route_fingerprint": runtime["route_fingerprint"],
                    "active_route_count": if is_apply { 1 } else { 0 },
                    "companion_process_running": true, "message": "test control response",
                    "unsupported_fields": [], "stats": null
                })),
                "error_code": (!accepted).then_some("ANDROID_CONTROL_LEASE_REQUIRED"),
                "error_message": (!accepted).then_some("protected apply requires current lease")
            });
            eprintln!("CONTROL_RESPONSE {response}");
            let bytes = serde_json::to_vec(&response).unwrap();
            stream.write_all(&u32::try_from(bytes.len()).unwrap().to_be_bytes()).await.unwrap();
            stream.write_all(&bytes).await.unwrap();
        });
        self.servers.lock().unwrap().insert(port, server);
        Ok(output)
    }
}

impl Drop for RecoveryRunner {
    fn drop(&mut self) {
        for (_, server) in self.servers.get_mut().unwrap().drain() {
            server.abort();
        }
    }
}

fn activation() -> AndroidNetworkActivation {
    test_activation("profile-manual", "203.0.113.10", ListenerId::new(), 8080)
}

async fn seed_lan(
    adapter: &AndroidAdbAdapter,
    activation: &AndroidNetworkActivation,
    state: AndroidRuntimeOwnerState,
    active_runtime: bool,
) -> AndroidRuntimeOwnerViewModel {
    let owner = AndroidRuntimeOwnerViewModel {
        serial: "MANUAL-A".into(), epoch: uuid::Uuid::new_v4(),
        profile_id: activation.profile.id.clone(), mode: AndroidRuntimeOwnerMode::Lan,
        source: AndroidRuntimeOwnerSource::Recovery, state,
        transition_reason: AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage,
        updated_at: chrono::Utc::now(),
    };
    let (_, runtime) = adapter.build_lan_runtime_for_owner(
        activation, &owner, Ipv4Addr::new(192, 168, 1, 10),
    ).await.unwrap();
    adapter.save_owner(owner.clone()).await.unwrap();
    let mut states = adapter.owner_states.lock().await;
    let stored = states.get_mut(&owner.serial).unwrap();
    stored.runtime_endpoints = runtime.endpoints.clone();
    stored.active_runtime = active_runtime.then_some(runtime);
    owner
}

#[tokio::test]
async fn lan_recovery_queries_never_apply_without_active_runtime_ownership() {
    use AndroidRuntimeOwnerState::*;
    for (state, runtime_present) in [
        (Active, false), (CleanupRequired, false), (CleanupRequired, true),
        (WaitingReconnect, true), (Uncertain, true), (Faulted, true), (StopFailed, true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let runner = Arc::new(RecoveryRunner::default());
        let mut adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
        adapter.lan_address = Arc::new(DesktopLan);
        let activation = activation();
        let owner = seed_lan(&adapter, &activation, state, runtime_present).await;
        let endpoints = adapter.network_runtime_endpoints(
            AndroidDeviceTarget { serial: owner.serial.clone() }, Some(activation),
        ).await.unwrap();
        assert!(runner.requests.lock().unwrap().is_empty(), "state={state:?}, runtime={runtime_present}");
        assert_eq!(adapter.runtime_owner_snapshot_for(&owner.serial).await.unwrap(), owner);
        assert_eq!(endpoints[0].proxy_host, "192.168.1.10");
        assert_ne!(endpoints[0].health, AndroidRuntimeEndpointHealth::Healthy);
    }
}

#[tokio::test]
async fn stopped_owner_waits_until_explicit_apply_and_gets_a_new_lease_epoch() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecoveryRunner::default());
    let mut adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    adapter.lan_address = Arc::new(DesktopLan);
    let activation = activation();
    let owner = seed_lan(&adapter, &activation, AndroidRuntimeOwnerState::Active, true).await;
    let target = AndroidDeviceTarget { serial: owner.serial.clone() };
    let stopped = adapter.network_status(target.clone()).await.unwrap();
    assert_eq!(stopped.state, AndroidNetworkState::Stopped);
    assert_eq!(adapter.runtime_owner_snapshot_for(&owner.serial).await.unwrap().state, AndroidRuntimeOwnerState::CleanupRequired);
    adapter.network_runtime_endpoints(target, Some(activation.clone())).await.unwrap();
    assert_eq!(runner.requests.lock().unwrap().len(), 1, "endpoint refresh must not restart the stopped VPN");
    let running = adapter.network_apply(
        AndroidRuntimeTarget { serial: owner.serial.clone(), expected_epoch: owner.epoch }, activation,
    ).await.unwrap();
    assert_eq!(running.state, AndroidNetworkState::Running);
    let current = adapter.runtime_owner_snapshot_for(&owner.serial).await.unwrap();
    assert_ne!(current.epoch, owner.epoch);
    assert_eq!(current.state, AndroidRuntimeOwnerState::Active);
    let requests = runner.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["operation"], "apply");
    assert_eq!(requests[1]["payload"]["control_lease"]["owner_epoch"], current.epoch.to_string());
    assert_eq!(requests[1]["payload"]["control_lease"]["timeout_millis"], 5_000);
}

#[tokio::test]
async fn active_lan_address_change_keeps_hot_apply_and_current_control_lease() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecoveryRunner::default());
    let mut adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    adapter.lan_address = Arc::new(DesktopLan);
    let activation = activation();
    let owner = seed_lan(&adapter, &activation, AndroidRuntimeOwnerState::Active, true).await;
    let endpoints = adapter.network_runtime_endpoints(
        AndroidDeviceTarget { serial: owner.serial.clone() }, Some(activation),
    ).await.unwrap();
    assert_eq!(endpoints[0].proxy_host, "192.168.1.20");
    assert_eq!(endpoints[0].health, AndroidRuntimeEndpointHealth::Healthy);
    let requests = runner.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["operation"], "apply");
    assert_eq!(requests[0]["payload"]["control_lease"]["owner_epoch"], owner.epoch.to_string());
}
