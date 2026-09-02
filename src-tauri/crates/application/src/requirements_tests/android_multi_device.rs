use super::*;

#[test]
fn device_and_runtime_targets_have_one_explicit_wire_identity() {
    let epoch = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    assert_eq!(
        serde_json::to_value(AndroidDeviceTarget {
            serial: "device-a".into(),
        })
        .unwrap(),
        serde_json::json!({ "serial": "device-a" })
    );
    assert_eq!(
        serde_json::to_value(AndroidRuntimeTarget {
            serial: "device-a".into(),
            expected_epoch: epoch,
        })
        .unwrap(),
        serde_json::json!({
            "serial": "device-a",
            "expected_epoch": "11111111-1111-4111-8111-111111111111"
        })
    );
}

#[test]
fn network_status_wire_always_contains_runtime_epoch() {
    let value = serde_json::to_value(AndroidNetworkStatusViewModel {
        serial: "device-a".into(),
        runtime_epoch: None,
        state: AndroidNetworkState::Stopped,
        state_text: "已停止".into(),
        ui_tone: UiTone::Neutral,
        verified: true,
        transport: AndroidControlTransport::LocalAbstractSocket,
        active_profile_id: None,
        active_profile_fingerprint: None,
        active_route_fingerprint: None,
        active_route_count: 0,
        companion_process_running: Some(false),
        message: "已停止".into(),
        unsupported_fields: Vec::new(),
        stats: None,
    })
    .unwrap();
    assert!(value.get("runtime_epoch").is_some());
    assert!(value["runtime_epoch"].is_null());
}

#[allow(dead_code)]
async fn port_contract_is_explicit_and_collection_owned<P: AndroidControlPort>(
    port: &P,
    device: AndroidDeviceTarget,
    runtime: AndroidRuntimeTarget,
    activation: AndroidNetworkActivation,
) -> AppResult<()> {
    let _ = port.package_list(device.clone()).await?;
    let _ = port
        .package_get(device.clone(), "com.example".into())
        .await?;
    let _ = port.companion_install(device.clone(), false).await?;
    let _ = port.vpn_open_consent(device.clone()).await?;
    let _ = port
        .network_start(device.clone(), activation.clone())
        .await?;
    let _ = port
        .network_apply(runtime.clone(), activation.clone())
        .await?;
    let status = port.network_status(device.clone()).await?;
    let _ = port
        .network_runtime_ready(device.clone(), &activation, &status)
        .await?;
    let _ = port
        .network_runtime_endpoints(device, Some(activation))
        .await?;
    let _ = port.network_stop(runtime.clone()).await?;
    let _ = port.emergency_restore(runtime).await?;
    let owners: Vec<AndroidRuntimeOwnerViewModel> = port.runtime_owners().await?;
    assert!(
        owners
            .windows(2)
            .all(|pair| pair[0].serial <= pair[1].serial)
    );
    Ok(())
}
