use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::routing::ProxyRouteTable;

pub(super) const VERSION: u8 = 5;
pub(super) const SUCCEEDED: u8 = 0;
pub(super) const GENERAL_FAILURE: u8 = 1;
pub(super) const COMMAND_NOT_SUPPORTED: u8 = 7;

#[derive(Clone, Debug)]
pub(super) enum Target {
    Address(SocketAddr),
    Domain(String, u16),
}

impl Target {
    pub(super) fn proxy_addresses<'a>(
        &self,
        routes: &'a ProxyRouteTable,
    ) -> Option<&'a [SocketAddr]> {
        match self {
            Self::Address(address) => routes.for_ip(address.ip(), address.port()),
            Self::Domain(domain, port) => routes.for_domain(domain, *port),
        }
    }

    pub(super) async fn resolve(&self) -> io::Result<Vec<SocketAddr>> {
        match self {
            Self::Address(address) => Ok(vec![*address]),
            Self::Domain(domain, port) => Ok(tokio::net::lookup_host((domain.as_str(), *port))
                .await?
                .collect()),
        }
    }
}

pub(super) async fn negotiate(client: &mut TcpStream) -> io::Result<(u8, Target)> {
    let mut greeting = [0_u8; 2];
    client.read_exact(&mut greeting).await?;
    if greeting[0] != VERSION || greeting[1] == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SOCKS5 greeting 无效",
        ));
    }

    let mut methods = vec![0_u8; usize::from(greeting[1])];
    client.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        client.write_all(&[VERSION, 0xff]).await?;
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "SOCKS5 客户端未提供 no-auth 方法",
        ));
    }
    client.write_all(&[VERSION, 0]).await?;

    let mut request = [0_u8; 4];
    client.read_exact(&mut request).await?;
    if request[0] != VERSION || request[2] != 0 {
        write_reply(client, GENERAL_FAILURE, None).await?;
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SOCKS5 request 无效",
        ));
    }
    Ok((request[1], read_target(client, request[3]).await?))
}

async fn read_target(client: &mut TcpStream, address_type: u8) -> io::Result<Target> {
    match address_type {
        1 => {
            let mut bytes = [0_u8; 6];
            client.read_exact(&mut bytes).await?;
            Ok(Target::Address(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3])),
                u16::from_be_bytes([bytes[4], bytes[5]]),
            )))
        }
        3 => {
            let length = client.read_u8().await?;
            let mut domain = vec![0_u8; usize::from(length)];
            client.read_exact(&mut domain).await?;
            let port = client.read_u16().await?;
            Ok(Target::Domain(
                String::from_utf8(domain).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "SOCKS5 域名非 UTF-8")
                })?,
                port,
            ))
        }
        4 => {
            let mut bytes = [0_u8; 18];
            client.read_exact(&mut bytes).await?;
            let mut ip = [0_u8; 16];
            ip.copy_from_slice(&bytes[..16]);
            Ok(Target::Address(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::from(ip)),
                u16::from_be_bytes([bytes[16], bytes[17]]),
            )))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("SOCKS5 地址类型不支持：{address_type}"),
        )),
    }
}

pub(super) fn parse_udp_request(datagram: &[u8]) -> io::Result<(Target, usize)> {
    if datagram.len() < 4 || datagram[0..2] != [0, 0] || datagram[2] != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SOCKS5 UDP header 无效",
        ));
    }
    match datagram[3] {
        1 if datagram.len() >= 10 => Ok((
            Target::Address(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(
                    datagram[4],
                    datagram[5],
                    datagram[6],
                    datagram[7],
                )),
                u16::from_be_bytes([datagram[8], datagram[9]]),
            )),
            10,
        )),
        3 if datagram.len() >= 5 => parse_udp_domain(datagram),
        4 if datagram.len() >= 22 => {
            let mut ip = [0_u8; 16];
            ip.copy_from_slice(&datagram[4..20]);
            Ok((
                Target::Address(SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(ip)),
                    u16::from_be_bytes([datagram[20], datagram[21]]),
                )),
                22,
            ))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SOCKS5 UDP 地址类型不支持",
        )),
    }
}

fn parse_udp_domain(datagram: &[u8]) -> io::Result<(Target, usize)> {
    let length = usize::from(datagram[4]);
    let end = 5 + length;
    if datagram.len() < end + 2 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "SOCKS5 UDP 域名不完整",
        ));
    }
    Ok((
        Target::Domain(
            String::from_utf8(datagram[5..end].to_vec()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "SOCKS5 UDP 域名非 UTF-8")
            })?,
            u16::from_be_bytes([datagram[end], datagram[end + 1]]),
        ),
        end + 2,
    ))
}

pub(super) fn encode_udp_response(source: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let mut response = vec![0, 0, 0];
    match source.ip() {
        IpAddr::V4(ip) => {
            response.push(1);
            response.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            response.push(4);
            response.extend_from_slice(&ip.octets());
        }
    }
    response.extend_from_slice(&source.port().to_be_bytes());
    response.extend_from_slice(payload);
    response
}

pub(super) async fn write_reply(
    stream: &mut TcpStream,
    status: u8,
    address: Option<SocketAddr>,
) -> io::Result<()> {
    let mut reply = vec![VERSION, status, 0];
    match address.unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)) {
        SocketAddr::V4(address) => {
            reply.push(1);
            reply.extend_from_slice(&address.ip().octets());
            reply.extend_from_slice(&address.port().to_be_bytes());
        }
        SocketAddr::V6(address) => {
            reply.push(4);
            reply.extend_from_slice(&address.ip().octets());
            reply.extend_from_slice(&address.port().to_be_bytes());
        }
    }
    stream.write_all(&reply).await
}
