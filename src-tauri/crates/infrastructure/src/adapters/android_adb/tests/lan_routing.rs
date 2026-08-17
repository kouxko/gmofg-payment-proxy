use super::super::*;

#[derive(Debug)]
struct FixedLanAddressProvider(std::net::Ipv4Addr);

impl DeviceLanAddressProvider for FixedLanAddressProvider {
    fn local_ipv4_for(&self, _: std::net::Ipv4Addr) -> Option<std::net::Ipv4Addr> {
        Some(self.0)
    }
}

#[tokio::test]
async fn same_subnet_listener_uses_lan_without_creating_reverse() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([AdbOutput {
            success: true,
            stdout: "30: wlan0 inet 10.0.35.195/23 brd 10.0.35.255 scope global wlan0\n36: tun0 inet 10.254.0.2/32 scope global tun0\n".into(),
            stderr: String::new(),
        }])),
    });
    let mut adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    adapter.lan_address = Arc::new(FixedLanAddressProvider("10.0.34.48".parse().unwrap()));
    *adapter.selected_serial.write().unwrap() = Some("SER123".into());
    let activation = test_activation("lan-profile", "203.0.113.10", ListenerId::new(), 16_127);

    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation, AndroidRuntimeOwnerSource::Start)
        .await
        .unwrap();

    assert_eq!(prepared.payload["routes"][0]["proxy_host"], "10.0.34.48");
    assert_eq!(prepared.payload["routes"][0]["proxy_port"], 16_127);
    assert!(!prepared.runtime.uses_adb_reverse);
    assert!(prepared.reverse.is_none());
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        &calls[0][2..],
        &["shell", "ip", "-o", "-4", "addr", "show", "scope", "global"]
    );
}

#[test]
fn lan_selection_requires_same_subnet_and_lan_listener() {
    let route = AndroidProxyRouteActivation {
        listener_id: "listener".into(),
        original_destination: "example.test".into(),
        original_ports: vec![443],
        desktop_listener_bind_address: "0.0.0.0".into(),
        desktop_listener_port: 443,
        allowed_client_cidrs: Vec::new(),
    };
    assert!(lan_endpoint_is_eligible(
        "10.0.34.48".parse().unwrap(),
        "10.0.35.195".parse().unwrap(),
        23,
        std::slice::from_ref(&route),
    ));
    assert!(!lan_endpoint_is_eligible(
        "10.0.34.48".parse().unwrap(),
        "10.0.35.195".parse().unwrap(),
        24,
        std::slice::from_ref(&route),
    ));
    let loopback_route = AndroidProxyRouteActivation {
        desktop_listener_bind_address: "127.0.0.1".into(),
        ..route
    };
    assert!(!lan_endpoint_is_eligible(
        "10.0.34.48".parse().unwrap(),
        "10.0.35.195".parse().unwrap(),
        23,
        &[loopback_route],
    ));
}
