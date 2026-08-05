use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, atomic::Ordering},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
};
use tun2proxy::CancellationToken;

use super::{RecordingProtection, no_auth};
use crate::routing::ProxyRouteTable;

#[test]
fn encodes_ipv6_udp_response() {
    let source = "[2001:db8::1]:53".parse().unwrap();
    let response = super::super::protocol::encode_udp_response(source, b"dns");
    assert_eq!(&response[..4], &[0, 0, 0, 4]);
    assert_eq!(&response[response.len() - 3..], b"dns");
}

#[tokio::test]
async fn associate_protects_both_families_and_rejects_attacker_first() {
    let echo_v4 = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let echo_v4_address = echo_v4.local_addr().unwrap();
    let echo_v4_task = echo_once(echo_v4);
    let echo_v6 = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).await.unwrap();
    let echo_v6_address = echo_v6.local_addr().unwrap();
    let echo_v6_task = echo_once(echo_v6);

    let protector = RecordingProtection::default();
    let calls = protector.calls.clone();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let socks_address = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        super::super::handle_client(
            stream,
            protector,
            Arc::new(ProxyRouteTable::default()),
            CancellationToken::new(),
            0,
            super::super::ServerLimits::default(),
        )
        .await
        .unwrap();
    });

    let mut control = TcpStream::connect(socks_address).await.unwrap();
    no_auth(&mut control).await;
    control
        .write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])
        .await
        .unwrap();
    let mut reply = [0_u8; 10];
    control.read_exact(&mut reply).await.unwrap();
    let relay_address = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        u16::from_be_bytes([reply[8], reply[9]]),
    );

    let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let attacker = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    attacker
        .send_to(b"invalid-first-packet", relay_address)
        .await
        .unwrap();
    assert_udp_round_trip(&client, relay_address, echo_v4_address, b"udp").await;
    assert_udp_round_trip(&client, relay_address, echo_v6_address, b"v6").await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    drop(control);
    echo_v4_task.await.unwrap();
    echo_v6_task.await.unwrap();
    server_task.await.unwrap();
}

fn echo_once(socket: UdpSocket) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut payload = [0_u8; 16];
        let (size, peer) = socket.recv_from(&mut payload).await.unwrap();
        socket.send_to(&payload[..size], peer).await.unwrap();
    })
}

async fn assert_udp_round_trip(
    client: &UdpSocket,
    relay: SocketAddr,
    target: SocketAddr,
    payload: &[u8],
) {
    let mut datagram = vec![0, 0, 0];
    match target.ip() {
        IpAddr::V4(ip) => {
            datagram.push(1);
            datagram.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            datagram.push(4);
            datagram.extend_from_slice(&ip.octets());
        }
    }
    datagram.extend_from_slice(&target.port().to_be_bytes());
    datagram.extend_from_slice(payload);
    client.send_to(&datagram, relay).await.unwrap();
    let mut response = [0_u8; 64];
    let size = client.recv(&mut response).await.unwrap();
    let (_, offset) = super::super::protocol::parse_udp_request(&response[..size]).unwrap();
    assert_eq!(&response[offset..size], payload);
}
