use std::collections::BTreeSet;

use serde_json::json;

use super::*;

#[test]
fn length_prefixed_protocol_rejects_truncation_and_oversize() {
    let request = AndroidControlRequest::new("status", json!({})).unwrap();
    let frame = encode_android_control_frame(&request).unwrap();
    assert_eq!(
        decode_android_control_frame::<AndroidControlRequest>(&frame).unwrap(),
        request
    );
    assert!(
        decode_android_control_frame::<AndroidControlRequest>(&frame[..frame.len() - 1]).is_err()
    );
    let mut oversized = vec![0; 4];
    oversized.copy_from_slice(
        &u32::try_from(ANDROID_CONTROL_MAX_FRAME_BYTES + 1)
            .unwrap()
            .to_be_bytes(),
    );
    assert!(decode_android_control_frame::<AndroidControlRequest>(&oversized).is_err());
}

#[test]
fn heartbeat_is_a_valid_control_protocol_operation() {
    AndroidControlRequest::new("heartbeat", serde_json::json!({"owner_epoch": "epoch"}))
        .expect("desktop lease renewal must be part of the versioned protocol");
}

#[test]
fn profile_rejects_companion_and_requires_confirmation_for_total_loss() {
    let profile = AndroidNetworkProfile {
        id: "danger".into(),
        name: "Danger".into(),
        target_applications: vec![AndroidTargetApplication {
            package_name: ANDROID_COMPANION_PACKAGE.into(),
            uid: 10_000,
            display_name: None,
        }],
        destination_targets: vec![AndroidDestinationTarget {
            cidr: "10.0.34.0/24".into(),
            ports: vec![443, 16_127],
        }],
        proxy_routes: Vec::new(),
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
        weak_network: WeakNetworkProfile {
            random_loss_basis_points: 10_000,
            ..WeakNetworkProfile::default()
        },
    };
    assert!(profile.validate().is_err());
    assert!(profile.requires_dangerous_confirmation());
}

#[test]
fn profile_accepts_multiple_destination_addresses_and_rejects_invalid_ranges() {
    let mut profile = AndroidNetworkProfile {
        id: "multiple-addresses".into(),
        name: "Multiple addresses".into(),
        target_applications: vec![AndroidTargetApplication {
            package_name: "com.example.client".into(),
            uid: 10_001,
            display_name: None,
        }],
        destination_targets: vec![
            AndroidDestinationTarget {
                cidr: "10.0.34.50".into(),
                ports: vec![16_127, 16_627],
            },
            AndroidDestinationTarget {
                cidr: "2001:db8::/32".into(),
                ports: Vec::new(),
            },
        ],
        proxy_routes: Vec::new(),
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
        weak_network: WeakNetworkProfile::default(),
    };
    profile.validate().expect("多个地址应通过校验");

    profile.destination_targets[1].cidr = "2001:db8::/129".into();
    assert!(profile.validate().is_err());
}

#[test]
fn companion_wire_status_is_projected_to_desktop_display_fields() {
    let response = serde_json::from_value::<AndroidControlResponse>(json!({
        "version": ANDROID_CONTROL_PROTOCOL_VERSION,
        "request_id": Uuid::nil(),
        "ok": true,
        "status": {
            "serial": "",
            "state": "running",
            "verified": true,
            "transport": "local_abstract_socket",
            "active_profile_id": "profile-1",
            "active_profile_fingerprint": "profile-fingerprint",
            "active_route_fingerprint": "route-fingerprint",
            "active_route_count": 2,
            "companion_process_running": true,
            "message": "native running",
            "unsupported_fields": ["serial"],
            "stats": null
        },
        "error_code": null,
        "error_message": null
    }))
    .expect("Companion machine status should not contain desktop display fields");

    let status = response.status.unwrap().into_view_model();
    assert_eq!(status.state_text, "运行中");
    assert_eq!(status.ui_tone, UiTone::Positive);
}

#[test]
fn companion_wire_status_rejects_desktop_display_fields() {
    let error = serde_json::from_value::<AndroidCompanionStatus>(json!({
        "serial": "",
        "state": "running",
        "state_text": "由设备伪造的文案",
        "verified": true,
        "transport": "local_abstract_socket",
        "active_profile_id": null,
        "active_profile_fingerprint": null,
        "active_route_fingerprint": null,
        "active_route_count": 0,
        "companion_process_running": true,
        "message": "native running",
        "unsupported_fields": ["serial"],
        "stats": {}
    }))
    .expect_err("Companion wire must not contain desktop-only display fields");

    assert!(error.to_string().contains("state_text"));
}

#[test]
fn nested_edit_defaults_are_owned_by_rust() {
    let mut profile = AndroidNetworkProfile {
        id: "defaults".into(),
        name: "Defaults".into(),
        target_applications: Vec::new(),
        destination_targets: Vec::new(),
        proxy_routes: Vec::new(),
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
        weak_network: WeakNetworkProfile::default(),
    };

    AndroidProfileEditIntent::SetBurstLossEnabled { enabled: true }.apply_defaults(&mut profile);
    AndroidProfileEditIntent::AddBlackoutWindow.apply_defaults(&mut profile);
    AndroidProfileEditIntent::AddTcpFlagDrop.apply_defaults(&mut profile);

    assert_eq!(
        profile.weak_network.burst_loss,
        Some(BurstLossProfile::default())
    );
    assert_eq!(
        profile.weak_network.blackout_windows,
        vec![BlackoutWindow::default()]
    );
    assert_eq!(
        profile.weak_network.nth_tcp_flag_drops,
        vec![NthTcpFlagDrop::default()]
    );
}
