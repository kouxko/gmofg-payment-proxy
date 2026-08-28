use std::collections::BTreeSet;

use crate::{
    AndroidNetworkProfile, AndroidProxyRoute, AndroidTargetApplication, ListenerId,
    WeakNetworkProfile,
};

fn profile_with_routes(proxy_routes: Vec<AndroidProxyRoute>) -> AndroidNetworkProfile {
    AndroidNetworkProfile {
        id: "route-validation".into(),
        name: "路由校验".into(),
        target_applications: vec![AndroidTargetApplication {
            package_name: "com.example.client".into(),
            uid: 10_001,
            display_name: None,
        }],
        destination_targets: Vec::new(),
        proxy_routes,
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
        weak_network: WeakNetworkProfile::default(),
    }
}

#[test]
fn missing_control_loss_policy_defaults_to_safe_stop() {
    let json = serde_json::json!({
        "id": "legacy-profile",
        "name": "旧方案",
        "target_applications": [{
            "package_name": "com.example.client",
            "uid": 10001,
            "display_name": null
        }],
        "destination_targets": [],
        "proxy_routes": [],
        "confirmed_shared_uids": [],
        "auto_resume_after_reboot": false,
        "weak_network": WeakNetworkProfile::default()
    });

    let profile: AndroidNetworkProfile = serde_json::from_value(json).expect("legacy profile");

    assert!(profile.stop_vpn_on_control_loss);
}

#[test]
fn proxy_route_uniqueness_is_normalized_per_destination_and_port() {
    let profile = profile_with_routes(vec![
        AndroidProxyRoute {
            destination: "Example.COM".into(),
            ports: vec![8_443, 9_443],
            listener_id: ListenerId::new(),
        },
        AndroidProxyRoute {
            destination: "example.com".into(),
            ports: vec![9_443],
            listener_id: ListenerId::new(),
        },
    ]);

    let error = profile
        .validate()
        .expect_err("同一目标端口不能因为 Listener 不同而重复");
    assert!(error.field_errors.contains_key("proxy_routes.1"));
}

#[test]
fn distinct_ports_may_route_to_distinct_listeners() {
    let profile = profile_with_routes(vec![
        AndroidProxyRoute {
            destination: "example.com".into(),
            ports: vec![8_443],
            listener_id: ListenerId::new(),
        },
        AndroidProxyRoute {
            destination: "example.com".into(),
            ports: vec![9_443],
            listener_id: ListenerId::new(),
        },
    ]);

    profile
        .validate()
        .expect("不同端口应允许转交给不同 Listener");
}

#[test]
fn equivalent_ipv6_cidr_and_hostname_routes_are_rejected() {
    for destinations in [
        ["2001:0db8::1", "2001:db8::1"],
        ["2001:db8:0:1::1/64", "2001:db8:0:1::abcd/64"],
        ["127.0.0.1", "127.0.0.1."],
        ["Example.COM.", "example.com"],
    ] {
        let profile = profile_with_routes(
            destinations
                .into_iter()
                .map(|destination| AndroidProxyRoute {
                    destination: destination.into(),
                    ports: vec![8_443],
                    listener_id: ListenerId::new(),
                })
                .collect(),
        );

        let error = profile
            .validate()
            .expect_err("语义等价的目标地址不能配置到同一端口");
        assert!(error.field_errors.contains_key("proxy_routes.1"));
    }
}
