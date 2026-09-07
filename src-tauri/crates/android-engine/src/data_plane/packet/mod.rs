//! 纯 IP/TCP/UDP 报文解析与变换。
//!
//! 本模块不持有运行时状态，只负责把原始 IP 包转换为数据面可用的元数据，
//! 并向上层提供校验和、MSS 与 PMTU 相关的确定性操作。

mod checksum;
mod pmtu;

use std::{
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Range,
};

use crate::{Direction, IpVersion, TcpFlag, TransportProtocol};

pub(super) use checksum::{
    checksum, clamp_existing_tcp_mss, refresh_checksums, transport_checksum_is_valid,
};
pub(super) use pmtu::{
    build_icmpv4_fragmentation_needed, build_icmpv6_packet_too_big, fragment_ipv4_packet,
};

#[derive(Clone, Debug)]
pub(super) struct ParsedPacket {
    pub(super) ip_version: IpVersion,
    pub(super) transport: TransportProtocol,
    pub(super) source_address: IpAddr,
    pub(super) destination_address: IpAddr,
    pub(super) source_port: Option<u16>,
    pub(super) destination_port: Option<u16>,
    pub(super) tcp_flags: BTreeSet<TcpFlag>,
    pub(super) transport_offset: usize,
    pub(super) payload_range: Range<usize>,
}

impl ParsedPacket {
    pub(super) fn parse(packet: &[u8]) -> Option<Self> {
        match packet.first()? >> 4 {
            4 => Self::parse_v4(packet),
            6 => Self::parse_v6(packet),
            _ => None,
        }
    }

    fn parse_v4(packet: &[u8]) -> Option<Self> {
        if packet.len() < 20 {
            return None;
        }
        let header_len = usize::from(packet[0] & 0x0f) * 4;
        if header_len < 20 || packet.len() < header_len {
            return None;
        }
        let source_address = IpAddr::V4(Ipv4Addr::new(
            packet[12], packet[13], packet[14], packet[15],
        ));
        let destination_address = IpAddr::V4(Ipv4Addr::new(
            packet[16], packet[17], packet[18], packet[19],
        ));
        Self::parse_transport(
            packet,
            IpVersion::V4,
            source_address,
            destination_address,
            packet[9],
            header_len,
        )
    }

    fn parse_v6(packet: &[u8]) -> Option<Self> {
        if packet.len() < 40 {
            return None;
        }
        // 首版只解析无扩展头的 TCP/UDP；遇到扩展头保持原样 fail-open。
        let source_address = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[8..24]).ok()?));
        let destination_address =
            IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[24..40]).ok()?));
        Self::parse_transport(
            packet,
            IpVersion::V6,
            source_address,
            destination_address,
            packet[6],
            40,
        )
    }

    fn parse_transport(
        packet: &[u8],
        ip_version: IpVersion,
        source_address: IpAddr,
        destination_address: IpAddr,
        protocol: u8,
        transport_offset: usize,
    ) -> Option<Self> {
        match protocol {
            6 if packet.len() >= transport_offset + 20 => {
                let tcp_header_len = usize::from(packet[transport_offset + 12] >> 4) * 4;
                let payload_start = transport_offset.checked_add(tcp_header_len)?;
                if tcp_header_len < 20 || payload_start > packet.len() {
                    return None;
                }
                Some(Self {
                    ip_version,
                    transport: TransportProtocol::Tcp,
                    source_address,
                    destination_address,
                    source_port: read_u16(packet, transport_offset),
                    destination_port: read_u16(packet, transport_offset + 2),
                    tcp_flags: tcp_flags(packet[transport_offset + 13]),
                    transport_offset,
                    payload_range: payload_start..packet.len(),
                })
            }
            17 if packet.len() >= transport_offset + 8 => Some(Self {
                ip_version,
                transport: TransportProtocol::Udp,
                source_address,
                destination_address,
                source_port: read_u16(packet, transport_offset),
                destination_port: read_u16(packet, transport_offset + 2),
                tcp_flags: BTreeSet::new(),
                transport_offset,
                payload_range: transport_offset + 8..packet.len(),
            }),
            _ => Some(Self {
                ip_version,
                transport: TransportProtocol::Other,
                source_address,
                destination_address,
                source_port: None,
                destination_port: None,
                tcp_flags: BTreeSet::new(),
                transport_offset,
                payload_range: packet.len()..packet.len(),
            }),
        }
    }

    pub(super) fn remote_address(&self, direction: Direction) -> IpAddr {
        match direction {
            Direction::Upload => self.destination_address,
            Direction::Download => self.source_address,
        }
    }

    pub(super) fn remote_port(&self, direction: Direction) -> Option<u16> {
        match direction {
            Direction::Upload => self.destination_port,
            Direction::Download => self.source_port,
        }
    }
}

fn tcp_flags(flags: u8) -> BTreeSet<TcpFlag> {
    let mut result = BTreeSet::new();
    if flags & 0x02 != 0 && flags & 0x10 != 0 {
        result.insert(TcpFlag::SynAck);
    } else if flags & 0x02 != 0 {
        result.insert(TcpFlag::Syn);
    } else if flags & 0x10 != 0 {
        result.insert(TcpFlag::Ack);
    }
    if flags & 0x01 != 0 {
        result.insert(TcpFlag::Fin);
    }
    if flags & 0x04 != 0 {
        result.insert(TcpFlag::Rst);
    }
    result
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}
