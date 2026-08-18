use super::super::*;
use super::{RecordingRunner, test_activation};
use async_trait::async_trait;
use intercept_proxy_application::{
    AndroidRuntimeEndpointHealth, AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerMode,
    AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason,
};
use intercept_proxy_domain::ListenerId;
use std::{net::Ipv4Addr, path::Path, sync::Arc};

#[derive(Debug)]
struct StaticLanAddress(Ipv4Addr);

impl DeviceLanAddressProvider for StaticLanAddress {
    fn local_ipv4_for(&self, _: Ipv4Addr) -> Option<Ipv4Addr> {
        Some(self.0)
    }
}

#[derive(Debug, Default)]
struct LanRunner {
    calls: std::sync::Mutex<Vec<Vec<String>>>,
}

#[async_trait]
impl AdbCommandRunner for LanRunner {
    async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(AdbOutput {
            success: true,
            stdout: "5: wlan0    inet 192.168.1.99/24 scope global wlan0\n".into(),
            stderr: String::new(),
        })
    }
}

fn endpoint(
    owner: &AndroidRuntimeOwnerViewModel,
    proxy_host: &str,
) -> AndroidRuntimeEndpointViewModel {
    AndroidRuntimeEndpointViewModel {
        serial: owner.serial.clone(),
        epoch: owner.epoch,
        mode: owner.mode,
        original_destination: "203.0.113.10".into(),
        original_ports: vec![443],
        resolved_original_ips: Vec::new(),
        listener_id: "00000000-0000-0000-0000-000000000123".into(),
        listener_name: "Test listener".into(),
        desktop_listener_port: 8080,
        proxy_host: proxy_host.into(),
        proxy_port: 8080,
        resolved_at: chrono::Utc::now(),
        health: AndroidRuntimeEndpointHealth::Healthy,
    }
}

fn owner(mode: AndroidRuntimeOwnerMode) -> AndroidRuntimeOwnerViewModel {
    AndroidRuntimeOwnerViewModel {
        serial: "OWNER-A".into(),
        epoch: uuid::Uuid::new_v4(),
        mode,
        profile_id: "profile-a".into(),
        state: AndroidRuntimeOwnerState::Active,
        source: AndroidRuntimeOwnerSource::Start,
        transition_reason: AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        updated_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn lan_address_change_reapplies_only_owner_and_failure_becomes_faulted() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(LanRunner::default());
    let mut adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    adapter.lan_address = Arc::new(StaticLanAddress("192.168.1.20".parse().unwrap()));
    *adapter.selected_serial.write().unwrap() = Some("SELECTED-B".into());
    let owner = owner(AndroidRuntimeOwnerMode::Lan);
    let old_endpoint = endpoint(&owner, "192.168.1.10");
    *adapter.runtime_endpoints.lock().await = vec![old_endpoint];
    adapter.save_owner(owner.clone()).await.unwrap();
    let activation = test_activation(
        "profile-a",
        "203.0.113.10",
        ListenerId::from_uuid(
            uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000123").unwrap(),
        ),
        8080,
    );

    let endpoints = adapter
        .network_runtime_endpoints(Some(activation))
        .await
        .unwrap();

    assert_eq!(endpoints[0].proxy_host, "192.168.1.20");
    assert_eq!(endpoints[0].health, AndroidRuntimeEndpointHealth::Faulted);
    let stored = adapter.runtime_owner_snapshot().await.unwrap();
    assert_eq!(stored.serial, "OWNER-A");
    assert_eq!(stored.epoch, owner.epoch);
    assert_eq!(stored.state, AndroidRuntimeOwnerState::Faulted);
    assert_eq!(
        stored.transition_reason,
        AndroidRuntimeOwnerTransitionReason::LanEndpointFaulted
    );
    let calls = runner.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .any(|args| args.iter().any(|arg| arg == "forward"))
    );
    assert!(
        calls
            .iter()
            .all(|args| args.get(1).is_some_and(|serial| serial == "OWNER-A"))
    );
}

#[tokio::test]
async fn healthy_reverse_endpoint_is_noop_even_when_another_device_is_selected() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("SELECTED-B".into());
    let owner = owner(AndroidRuntimeOwnerMode::AdbReverse);
    let runtime_endpoint = endpoint(&owner, "127.0.0.1");
    *adapter.runtime_endpoints.lock().await = vec![runtime_endpoint.clone()];
    adapter.save_owner(owner).await.unwrap();

    let endpoints = adapter.network_runtime_endpoints(None).await.unwrap();

    assert_eq!(endpoints, vec![runtime_endpoint]);
    assert!(runner.calls.lock().unwrap().is_empty());
}
