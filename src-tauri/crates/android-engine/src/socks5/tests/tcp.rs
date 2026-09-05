use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::atomic::Ordering,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tun2proxy::CancellationToken;

use super::{
    RecordingProtection, ipv4_connect_request, no_auth, route_table, unreachable_original,
};
use crate::{DestinationTarget, routing::ProxyRouteTable};

#[tokio::test]
async fn unknown_ip_on_a_unique_domain_port_connects_original_instead_of_proxy() {
    use crate::{
        InstalledApplication, NetworkProfile, ProxyRoute, ProxyRuntimeConfiguration,
        ResolvedProxyRoute, TargetApplication, WeakNetworkProfile,
    };

    let original_server = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let original = original_server.local_addr().unwrap();
    let proxy_server = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let proxy = proxy_server.local_addr().unwrap();
    let installed = InstalledApplication {
        package_name: "com.example.target".into(),
        uid: 10_001,
    };
    let profile = NetworkProfile {
        id: "strict-domain-route".into(),
        name: "精确域名路由".into(),
        target_applications: vec![TargetApplication {
            package_name: installed.package_name.clone(),
            uid: installed.uid,
        }],
        destination_targets: Vec::new(),
        proxy_routes: vec![ProxyRoute {
            listener_id: "proxy".into(),
            destination: "configured.example".into(),
            ports: vec![original.port()],
        }],
        confirmed_shared_uids: Default::default(),
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
        weak_network: WeakNetworkProfile::default(),
    }
    .validate_for_start(&[installed])
    .unwrap();
    let runtime = ProxyRuntimeConfiguration {
        routes: vec![ResolvedProxyRoute {
            listener_id: "proxy".into(),
            original_destination: "configured.example".into(),
            original_ports: vec![original.port()],
            resolved_original_ips: vec!["192.0.2.1".parse().unwrap()],
            proxy_host: proxy.ip().to_string(),
            proxy_port: proxy.port(),
        }],
    };
    let routes = std::sync::Arc::new(ProxyRouteTable::compile(&profile, &runtime).await.unwrap());
    let original_task = tokio::spawn(async move {
        let (mut stream, _) = original_server.accept().await.unwrap();
        let mut request = [0; 4];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"PING");
        eprintln!("strict-routing original={original} received={request:?} response=ORIG");
        stream.write_all(b"ORIG").await.unwrap();
    });
    let proxy_task = tokio::spawn(async move {
        let (mut stream, _) = proxy_server.accept().await.unwrap();
        let mut request = [0; 4];
        stream.read_exact(&mut request).await.unwrap();
        stream.write_all(b"PROX").await.unwrap();
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        assert_relay(original, routes, b"PING", b"ORIG"),
    )
    .await
    .expect("SOCKS5 应直连未匹配的原始目标");
    original_task.await.unwrap();
    proxy_task.abort();
}

#[tokio::test]
async fn connect_protects_outbound_socket_before_relay() {
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut payload = [0_u8; 4];
        stream.read_exact(&mut payload).await.unwrap();
        stream.write_all(&payload).await.unwrap();
    });

    let protector = RecordingProtection::default();
    let calls = protector.calls.clone();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        super::super::handle_client(
            stream,
            protector,
            std::sync::Arc::new(ProxyRouteTable::default()),
            CancellationToken::new(),
            0,
            super::super::ServerLimits::default(),
        )
        .await
        .unwrap();
    });

    let mut client = TcpStream::connect(address).await.unwrap();
    no_auth(&mut client).await;
    client
        .write_all(&ipv4_connect_request(upstream_address))
        .await
        .unwrap();
    let mut reply = [0_u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], super::super::protocol::SUCCEEDED);
    client.write_all(b"ping").await.unwrap();
    let mut echoed = [0_u8; 4];
    client.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"ping");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(client);
    upstream_task.await.unwrap();
    server_task.await.unwrap();
}

#[tokio::test]
async fn matched_original_target_uses_listener_when_original_is_unreachable() {
    let listener_fixture = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let listener_address = listener_fixture.local_addr().unwrap();
    let fixture_task = tokio::spawn(async move {
        let (mut stream, _) = listener_fixture.accept().await.unwrap();
        let mut payload = [0_u8; 4];
        stream.read_exact(&mut payload).await.unwrap();
        assert_eq!(&payload, b"PING");
        stream.write_all(b"PONG").await.unwrap();
    });
    let original = unreachable_original(61_627);
    let routes = route_table(
        original,
        listener_address,
        vec![DestinationTarget {
            cidr: "192.0.2.0/24".into(),
            ports: vec![443],
        }],
    )
    .await;
    assert_relay(original, routes, b"PING", b"PONG").await;
    fixture_task.await.unwrap();
}

#[tokio::test]
async fn unmatched_target_still_connects_original_directly() {
    let fixture = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let original = fixture.local_addr().unwrap();
    let fixture_task = tokio::spawn(async move {
        let (mut stream, _) = fixture.accept().await.unwrap();
        let mut payload = [0_u8; 4];
        stream.read_exact(&mut payload).await.unwrap();
        stream.write_all(&payload).await.unwrap();
    });
    let routes = route_table(unreachable_original(61_627), original, Vec::new()).await;
    assert_relay(original, routes, b"pass", b"pass").await;
    fixture_task.await.unwrap();
}

async fn assert_relay(
    target: SocketAddr,
    routes: std::sync::Arc<ProxyRouteTable>,
    request: &[u8],
    expected: &[u8],
) {
    let protector = RecordingProtection::default();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        super::super::handle_client(
            stream,
            protector,
            routes,
            CancellationToken::new(),
            0,
            super::super::ServerLimits::default(),
        )
        .await
        .unwrap();
    });
    let mut client = TcpStream::connect(address).await.unwrap();
    no_auth(&mut client).await;
    client
        .write_all(&ipv4_connect_request(target))
        .await
        .unwrap();
    let mut reply = [0_u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], super::super::protocol::SUCCEEDED);
    client.write_all(request).await.unwrap();
    let mut response = vec![0_u8; expected.len()];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(response, expected);
    drop(client);
    server_task.await.unwrap();
}
