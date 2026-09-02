use std::{
    collections::BTreeSet,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    os::fd::AsRawFd,
};

use tokio::{
    io::AsyncReadExt,
    net::{TcpStream, UdpSocket},
};
use tun2proxy::CancellationToken;

use super::protocol;
use crate::data_plane::{MAX_IP_PACKET_SIZE, SocketProtection};

pub(super) async fn associate<P>(
    mut control: TcpStream,
    protector: P,
    cancellation: CancellationToken,
) -> io::Result<()>
where
    P: SocketProtection,
{
    // 客户端 relay 与外连 socket 必须分离。若复用一个 0.0.0.0 socket，本机任意进程
    // 都可能抢先发送一个数据报成为“客户端”，后续远端数据还会被错误转发给攻击者。
    let client_relay = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let outbound_v4 = protected_udp_socket(IpAddr::V4(Ipv4Addr::UNSPECIFIED), &protector)?;
    let outbound_v6 = protected_udp_socket(IpAddr::V6(Ipv6Addr::UNSPECIFIED), &protector)?;
    protocol::write_reply(
        &mut control,
        protocol::SUCCEEDED,
        Some(client_relay.local_addr()?),
    )
    .await?;

    let mut client_address = None;
    let mut allowed_remote_addresses = BTreeSet::new();
    let mut client_datagram = vec![0_u8; MAX_IP_PACKET_SIZE];
    let mut datagram_v4 = vec![0_u8; MAX_IP_PACKET_SIZE];
    let mut datagram_v6 = vec![0_u8; MAX_IP_PACKET_SIZE];
    let mut control_probe = [0_u8; 1];
    loop {
        tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            read = control.read(&mut control_probe) => {
                if read? == 0 {
                    return Ok(());
                }
            }
            received = client_relay.recv_from(&mut client_datagram) => {
                let (size, source) = received?;
                if client_address.is_some_and(|client| client != source) {
                    continue;
                }
                // 只有完整、合法的 SOCKS UDP request 才能认领客户端地址。
                let Ok((target, payload_offset)) =
                    protocol::parse_udp_request(&client_datagram[..size])
                else {
                    continue;
                };
                client_address.get_or_insert(source);
                for destination in target.resolve().await? {
                    let result = if destination.is_ipv4() {
                        outbound_v4
                            .send_to(&client_datagram[payload_offset..size], destination)
                            .await
                    } else {
                        outbound_v6
                            .send_to(&client_datagram[payload_offset..size], destination)
                            .await
                    };
                    if result.is_ok() {
                        allowed_remote_addresses.insert(destination);
                        break;
                    }
                }
            }
            received = outbound_v4.recv_from(&mut datagram_v4) => {
                let (size, source) = received?;
                relay_response(
                    &client_relay,
                    client_address,
                    &allowed_remote_addresses,
                    source,
                    &datagram_v4[..size],
                ).await?;
            }
            received = outbound_v6.recv_from(&mut datagram_v6) => {
                let (size, source) = received?;
                relay_response(
                    &client_relay,
                    client_address,
                    &allowed_remote_addresses,
                    source,
                    &datagram_v6[..size],
                ).await?;
            }
        }
    }
}

fn protected_udp_socket(
    address: IpAddr,
    protector: &impl SocketProtection,
) -> io::Result<UdpSocket> {
    let socket = std::net::UdpSocket::bind((address, 0))?;
    socket.set_nonblocking(true)?;
    protector.protect(socket.as_raw_fd())?;
    UdpSocket::from_std(socket)
}

async fn relay_response(
    client_relay: &UdpSocket,
    client_address: Option<std::net::SocketAddr>,
    allowed_remote_addresses: &BTreeSet<std::net::SocketAddr>,
    source: std::net::SocketAddr,
    payload: &[u8],
) -> io::Result<()> {
    if allowed_remote_addresses.contains(&source)
        && let Some(client) = client_address
    {
        let response = protocol::encode_udp_response(source, payload);
        client_relay.send_to(&response, client).await?;
    }
    Ok(())
}
