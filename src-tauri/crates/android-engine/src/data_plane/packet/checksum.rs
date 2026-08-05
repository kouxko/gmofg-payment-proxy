//! 传输层与 IP 层校验和，以及 TCP MSS 原位变换。

use crate::{IpVersion, PathMtuAction, TransportProtocol};

use super::ParsedPacket;

pub(crate) fn transport_checksum_is_valid(packet: &[u8], metadata: &ParsedPacket) -> bool {
    if metadata.transport == TransportProtocol::Other {
        return true;
    }
    let checksum_offset = match metadata.transport {
        TransportProtocol::Tcp => metadata.transport_offset + 16,
        TransportProtocol::Udp => metadata.transport_offset + 6,
        TransportProtocol::Other => return true,
    };
    if checksum_offset + 2 > packet.len() {
        return false;
    }
    if metadata.transport == TransportProtocol::Udp
        && metadata.ip_version == IpVersion::V4
        && packet[checksum_offset..checksum_offset + 2] == [0, 0]
    {
        return true;
    }

    let segment = &packet[metadata.transport_offset..];
    let mut pseudo = Vec::with_capacity(40 + segment.len());
    match metadata.ip_version {
        IpVersion::V4 => {
            pseudo.extend_from_slice(&packet[12..20]);
            pseudo.push(0);
            pseudo.push(packet[9]);
            let Ok(length) = u16::try_from(segment.len()) else {
                return false;
            };
            pseudo.extend_from_slice(&length.to_be_bytes());
        }
        IpVersion::V6 => {
            pseudo.extend_from_slice(&packet[8..40]);
            let Ok(length) = u32::try_from(segment.len()) else {
                return false;
            };
            pseudo.extend_from_slice(&length.to_be_bytes());
            pseudo.extend_from_slice(&[0, 0, 0, packet[6]]);
        }
    }
    pseudo.extend_from_slice(segment);
    checksum(&pseudo) == 0
}

pub(crate) fn clamp_existing_tcp_mss(
    packet: &mut [u8],
    metadata: &ParsedPacket,
    action: PathMtuAction,
) -> bool {
    let PathMtuAction::ClampMss(mss) = action else {
        return false;
    };
    if metadata.transport != TransportProtocol::Tcp {
        return false;
    }
    let header_end = metadata.payload_range.start;
    let mut offset = metadata.transport_offset + 20;
    while offset < header_end {
        match packet[offset] {
            0 => break,
            1 => offset += 1,
            2 if offset + 4 <= header_end && packet[offset + 1] == 4 => {
                let changed = packet[offset + 2..offset + 4] != mss.to_be_bytes();
                if changed {
                    packet[offset + 2..offset + 4].copy_from_slice(&mss.to_be_bytes());
                }
                return changed;
            }
            _ if offset + 1 < header_end => {
                let length = usize::from(packet[offset + 1]);
                if length < 2 || offset + length > header_end {
                    break;
                }
                offset += length;
            }
            _ => break,
        }
    }
    false
}

pub(crate) fn refresh_checksums(packet: &mut [u8], metadata: &ParsedPacket) {
    match metadata.ip_version {
        IpVersion::V4 => {
            let header_len = usize::from(packet[0] & 0x0f) * 4;
            packet[10..12].fill(0);
            let value = checksum(&packet[..header_len]);
            packet[10..12].copy_from_slice(&value.to_be_bytes());
            refresh_transport_checksum_v4(packet, metadata);
        }
        IpVersion::V6 => refresh_transport_checksum_v6(packet, metadata),
    }
}

fn refresh_transport_checksum_v4(packet: &mut [u8], metadata: &ParsedPacket) {
    let Some(checksum_offset) = transport_checksum_offset(metadata) else {
        return;
    };
    if checksum_offset + 2 > packet.len() || packet.len() < 20 {
        return;
    }
    packet[checksum_offset..checksum_offset + 2].fill(0);
    let segment_len = packet.len().saturating_sub(metadata.transport_offset);
    let Ok(segment_len) = u16::try_from(segment_len) else {
        return;
    };
    let mut pseudo = Vec::with_capacity(12 + usize::from(segment_len));
    pseudo.extend_from_slice(&packet[12..20]);
    pseudo.push(0);
    pseudo.push(packet[9]);
    pseudo.extend_from_slice(&segment_len.to_be_bytes());
    pseudo.extend_from_slice(&packet[metadata.transport_offset..]);
    let value = checksum(&pseudo);
    packet[checksum_offset..checksum_offset + 2].copy_from_slice(&value.to_be_bytes());
}

fn refresh_transport_checksum_v6(packet: &mut [u8], metadata: &ParsedPacket) {
    let Some(checksum_offset) = transport_checksum_offset(metadata) else {
        return;
    };
    if checksum_offset + 2 > packet.len() || packet.len() < 40 {
        return;
    }
    packet[checksum_offset..checksum_offset + 2].fill(0);
    let Ok(segment_len) = u32::try_from(packet.len().saturating_sub(metadata.transport_offset))
    else {
        return;
    };
    let mut pseudo = Vec::with_capacity(40 + segment_len as usize);
    pseudo.extend_from_slice(&packet[8..40]);
    pseudo.extend_from_slice(&segment_len.to_be_bytes());
    pseudo.extend_from_slice(&[0, 0, 0, packet[6]]);
    pseudo.extend_from_slice(&packet[metadata.transport_offset..]);
    let value = checksum(&pseudo);
    packet[checksum_offset..checksum_offset + 2].copy_from_slice(&value.to_be_bytes());
}

fn transport_checksum_offset(metadata: &ParsedPacket) -> Option<usize> {
    match metadata.transport {
        TransportProtocol::Tcp => Some(metadata.transport_offset + 16),
        TransportProtocol::Udp => Some(metadata.transport_offset + 6),
        TransportProtocol::Other => None,
    }
}

pub(crate) fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0_u32;
    for pair in bytes.chunks(2) {
        let word = if pair.len() == 2 {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from(pair[0]) << 8
        };
        sum = sum.wrapping_add(u32::from(word));
        while sum > 0xffff {
            sum = (sum & 0xffff) + (sum >> 16);
        }
    }
    !u16::try_from(sum).unwrap_or(u16::MAX)
}
