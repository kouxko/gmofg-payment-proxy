mod lifecycle;
mod tcp;
mod udp;

use std::{
    collections::BTreeSet,
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::{SocketProtection, protocol};
use crate::{
    DestinationTarget, InstalledApplication, NetworkProfile, ProxyRoute, ProxyRuntimeConfiguration,
    ResolvedProxyRoute, TargetApplication, WeakNetworkProfile, routing::ProxyRouteTable,
};

#[derive(Clone, Debug, Default)]
struct RecordingProtection {
    calls: Arc<AtomicUsize>,
}

impl SocketProtection for RecordingProtection {
    fn protect(&self, _fd: i32) -> io::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn notify_failure(&self, _message: &str) {}
}

async fn no_auth(client: &mut TcpStream) {
    client
        .write_all(&[protocol::VERSION, 1, 0])
        .await
        .expect("发送 SOCKS5 greeting");
    let mut reply = [0_u8; 2];
    client.read_exact(&mut reply).await.expect("读取 greeting");
    assert_eq!(reply, [protocol::VERSION, 0]);
}

async fn route_table(
    original: SocketAddr,
    proxy: SocketAddr,
    destination_targets: Vec<DestinationTarget>,
) -> Arc<ProxyRouteTable> {
    let installed = InstalledApplication {
        package_name: "com.example.target".into(),
        uid: 10_001,
    };
    let profile = NetworkProfile {
        id: "transparent-route-test".into(),
        name: "透明路由".into(),
        target_applications: vec![TargetApplication {
            package_name: installed.package_name.clone(),
            uid: installed.uid,
        }],
        destination_targets,
        proxy_routes: vec![ProxyRoute {
            listener_id: "fixture-listener".into(),
            destination: original.ip().to_string(),
            ports: vec![original.port()],
        }],
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
        weak_network: WeakNetworkProfile::default(),
    }
    .validate_for_start(&[installed])
    .unwrap();
    let runtime = ProxyRuntimeConfiguration {
        routes: vec![ResolvedProxyRoute {
            listener_id: "fixture-listener".into(),
            original_destination: original.ip().to_string(),
            original_ports: vec![original.port()],
            resolved_original_ips: Vec::new(),
            proxy_host: proxy.ip().to_string(),
            proxy_port: proxy.port(),
        }],
    };
    Arc::new(ProxyRouteTable::compile(&profile, &runtime).await.unwrap())
}

fn ipv4_connect_request(target: SocketAddr) -> Vec<u8> {
    let SocketAddr::V4(target) = target else {
        panic!("测试目标应为 IPv4");
    };
    let mut request = vec![protocol::VERSION, 1, 0, 1];
    request.extend_from_slice(&target.ip().octets());
    request.extend_from_slice(&target.port().to_be_bytes());
    request
}

fn unreachable_original(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), port)
}
