use std::{collections::BTreeSet, io, net::IpAddr};

use crate::{
    InstalledApplication, NetworkProfile, ProxyRoute, ProxyRuntimeConfiguration,
    ResolvedProxyRoute, TargetApplication, ValidatedProfile, WeakNetworkProfile,
};

use super::ProxyRouteTable;

fn profile(routes: Vec<ProxyRoute>) -> ValidatedProfile {
    let installed = InstalledApplication {
        package_name: "com.example.target".into(),
        signing_sha256: "AA".into(),
        uid: 10_001,
    };
    NetworkProfile {
        id: "route-test".into(),
        name: "路由".into(),
        target_applications: vec![TargetApplication {
            package_name: installed.package_name.clone(),
            signing_sha256: installed.signing_sha256.clone(),
            uid: installed.uid,
        }],
        destination_targets: Vec::new(),
        proxy_routes: routes,
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        weak_network: WeakNetworkProfile::default(),
    }
    .validate_for_start(&[installed])
    .unwrap()
}

#[tokio::test]
async fn routes_multiple_original_targets_to_independent_runtime_endpoints() {
    let profile = profile(vec![
        ProxyRoute {
            listener_id: "transaction".into(),
            destination: "127.0.0.1".into(),
            ports: vec![18_080],
        },
        ProxyRoute {
            listener_id: "dll".into(),
            destination: "10.0.0.0/8".into(),
            ports: vec![18_081],
        },
    ]);
    let runtime = ProxyRuntimeConfiguration {
        routes: vec![
            ResolvedProxyRoute {
                listener_id: "transaction".into(),
                original_destination: "127.0.0.1".into(),
                original_ports: vec![18_080],
                resolved_original_ips: Vec::new(),
                proxy_host: "127.0.0.1".into(),
                proxy_port: 36_627,
            },
            ResolvedProxyRoute {
                listener_id: "dll".into(),
                original_destination: "10.0.0.0/8".into(),
                original_ports: vec![18_081],
                resolved_original_ips: Vec::new(),
                proxy_host: "127.0.0.1".into(),
                proxy_port: 26_127,
            },
        ],
    };
    let table = ProxyRouteTable::compile(&profile, &runtime).await.unwrap();
    assert_eq!(
        table.for_ip("127.0.0.1".parse().unwrap(), 18_080).unwrap()[0],
        "127.0.0.1:36627".parse().unwrap()
    );
    assert_eq!(
        table.for_ip("10.2.3.4".parse().unwrap(), 18_081).unwrap()[0],
        "127.0.0.1:26127".parse().unwrap()
    );
    assert!(table.for_ip("192.0.2.1".parse().unwrap(), 443).is_none());
}

#[tokio::test]
async fn domain_route_matches_tun_ip_after_startup_resolution() {
    let profile = profile(vec![ProxyRoute {
        listener_id: "localhost".into(),
        destination: "localhost".into(),
        ports: vec![443],
    }]);
    let runtime = ProxyRuntimeConfiguration {
        routes: vec![ResolvedProxyRoute {
            listener_id: "localhost".into(),
            original_destination: "localhost".into(),
            original_ports: vec![443],
            resolved_original_ips: Vec::new(),
            proxy_host: "127.0.0.1".into(),
            proxy_port: 18_080,
        }],
    };
    let table = ProxyRouteTable::compile(&profile, &runtime).await.unwrap();
    assert_eq!(
        table.for_ip(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 443),
        Some(&["127.0.0.1:18080".parse().unwrap()][..])
    );
}

#[tokio::test]
async fn missing_runtime_route_blocks_start_instead_of_bypassing_proxy() {
    let profile = profile(vec![ProxyRoute {
        listener_id: "required".into(),
        destination: "127.0.0.1".into(),
        ports: vec![443],
    }]);
    let error = ProxyRouteTable::compile(&profile, &ProxyRuntimeConfiguration::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("不一致"));
}
