//! IPv4 分片与 ICMP 路径 MTU 信号构造。

use super::checksum::checksum;

/// 将一个尚未分片的 IPv4 数据报拆成不超过 `mtu` 的标准分片。
///
/// 除最后一片外，负载长度必须是 8 的倍数；传输层校验和属于重组后的完整报文，
/// 因此这里仅重算各分片的 IPv4 头校验和，不能改写 TCP/UDP 校验和。
pub(crate) fn fragment_ipv4_packet(packet: &[u8], mtu: u16) -> Option<Vec<Vec<u8>>> {
    if packet.len() < 20 || packet[0] >> 4 != 4 {
        return None;
    }
    let header_len = usize::from(packet[0] & 0x0f) * 4;
    let mtu = usize::from(mtu);
    if header_len < 20 || header_len >= mtu || packet.len() <= mtu {
        return None;
    }
    let flags_offset = u16::from_be_bytes([packet[6], packet[7]]);
    // 已经带非零 offset 的包不再次分片；数据面保持 fail-open 并暴露未实施计数。
    if flags_offset & 0x1fff != 0 {
        return None;
    }
    let fragment_payload_len = ((mtu - header_len) / 8) * 8;
    if fragment_payload_len == 0 {
        return None;
    }
    let payload = &packet[header_len..];
    let mut fragments = Vec::new();
    for (index, chunk) in payload.chunks(fragment_payload_len).enumerate() {
        let offset_bytes = index.checked_mul(fragment_payload_len)?;
        let offset_units = u16::try_from(offset_bytes / 8).ok()?;
        let more_fragments = offset_bytes + chunk.len() < payload.len();
        let mut fragment = Vec::with_capacity(header_len + chunk.len());
        fragment.extend_from_slice(&packet[..header_len]);
        fragment.extend_from_slice(chunk);
        let total_len = u16::try_from(fragment.len()).ok()?;
        fragment[2..4].copy_from_slice(&total_len.to_be_bytes());
        // 保留 reserved 位，清除 DF；除最后一片外设置 MF。
        let mut new_flags_offset = flags_offset & 0x8000;
        if more_fragments {
            new_flags_offset |= 0x2000;
        }
        new_flags_offset |= offset_units & 0x1fff;
        fragment[6..8].copy_from_slice(&new_flags_offset.to_be_bytes());
        fragment[10..12].fill(0);
        let header_checksum = checksum(&fragment[..header_len]);
        fragment[10..12].copy_from_slice(&header_checksum.to_be_bytes());
        fragments.push(fragment);
    }
    (fragments.len() > 1).then_some(fragments)
}

/// 构造 `ICMPv4` Destination Unreachable / Fragmentation Needed（Type 3 Code 4）。
pub(crate) fn build_icmpv4_fragmentation_needed(packet: &[u8], mtu: u16) -> Option<Vec<u8>> {
    if packet.len() < 20 || packet[0] >> 4 != 4 {
        return None;
    }
    let original_header_len = usize::from(packet[0] & 0x0f) * 4;
    if original_header_len < 20 || original_header_len > packet.len() {
        return None;
    }
    let quote_len = packet.len().min(original_header_len.saturating_add(8));
    let total_len = 20_usize.checked_add(8)?.checked_add(quote_len)?;
    let total_len_u16 = u16::try_from(total_len).ok()?;
    let mut response = vec![0_u8; total_len];
    response[0] = 0x45;
    response[2..4].copy_from_slice(&total_len_u16.to_be_bytes());
    response[8] = 64;
    response[9] = 1;
    // ICMP 由路径节点返回原发送方；在透明模拟中使用原目的地址作为节点地址。
    response[12..16].copy_from_slice(&packet[16..20]);
    response[16..20].copy_from_slice(&packet[12..16]);
    response[20] = 3;
    response[21] = 4;
    response[26..28].copy_from_slice(&mtu.to_be_bytes());
    response[28..].copy_from_slice(&packet[..quote_len]);
    let icmp_checksum = checksum(&response[20..]);
    response[22..24].copy_from_slice(&icmp_checksum.to_be_bytes());
    let ip_checksum = checksum(&response[..20]);
    response[10..12].copy_from_slice(&ip_checksum.to_be_bytes());
    Some(response)
}

/// 构造 `ICMPv6` Packet Too Big（Type 2 Code 0）。
pub(crate) fn build_icmpv6_packet_too_big(packet: &[u8], mtu: u16) -> Option<Vec<u8>> {
    if packet.len() < 40 || packet[0] >> 4 != 6 {
        return None;
    }
    // IPv6 要求尽量引用触发包，但整个 ICMPv6 报文不得超过最小 IPv6 MTU 1280。
    let quote_len = packet.len().min(1_280 - 40 - 8);
    let payload_len = 8_usize.checked_add(quote_len)?;
    let payload_len_u16 = u16::try_from(payload_len).ok()?;
    let mut response = vec![0_u8; 40 + payload_len];
    response[0] = 0x60;
    response[4..6].copy_from_slice(&payload_len_u16.to_be_bytes());
    response[6] = 58;
    response[7] = 64;
    response[8..24].copy_from_slice(&packet[24..40]);
    response[24..40].copy_from_slice(&packet[8..24]);
    response[40] = 2;
    response[41] = 0;
    response[44..48].copy_from_slice(&u32::from(mtu).to_be_bytes());
    response[48..].copy_from_slice(&packet[..quote_len]);

    let mut pseudo = Vec::with_capacity(40 + payload_len);
    pseudo.extend_from_slice(&response[8..40]);
    pseudo.extend_from_slice(&u32::try_from(payload_len).ok()?.to_be_bytes());
    pseudo.extend_from_slice(&[0, 0, 0, 58]);
    pseudo.extend_from_slice(&response[40..]);
    let icmp_checksum = checksum(&pseudo);
    response[42..44].copy_from_slice(&icmp_checksum.to_be_bytes());
    Some(response)
}
