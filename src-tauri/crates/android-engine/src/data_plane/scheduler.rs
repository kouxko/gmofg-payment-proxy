//! 弱网决策到定时发送队列的转换。

use std::{
    cmp::Ordering,
    collections::{BTreeSet, BinaryHeap},
    future::Future,
    io,
    sync::atomic::Ordering as AtomicOrdering,
    time::{Duration, Instant},
};

use tokio::{
    sync::mpsc,
    time::{Instant as TokioInstant, sleep_until},
};
use tun2proxy::CancellationToken;

use crate::{
    Direction, FailOpenEngine, IpVersion, PacketContext, PathMtuAction, TransportProtocol,
};

use super::{
    RUNTIME_STATS,
    packet::{
        ParsedPacket, build_icmpv4_fragmentation_needed, build_icmpv6_packet_too_big,
        clamp_existing_tcp_mss, fragment_ipv4_packet, refresh_checksums,
    },
    record_runtime_error,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PacketRoute {
    Forward,
    Reverse,
}

#[derive(Debug, Eq)]
pub(super) struct ScheduledPacket {
    pub(super) due: TokioInstant,
    pub(super) sequence: u64,
    pub(super) copies: u8,
    pub(super) route: PacketRoute,
    pub(super) packet: Vec<u8>,
}

impl Ord for ScheduledPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap 默认弹出“最大”元素，故反转时间与序号，使最早到期、最小序号优先。
        other
            .due
            .cmp(&self.due)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for ScheduledPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ScheduledPacket {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due && self.sequence == other.sequence
    }
}

pub(super) fn prepare_scheduled_packets(
    mut packet: Vec<u8>,
    direction: Direction,
    engine: &FailOpenEngine,
    started_at: Instant,
    sequence: u64,
) -> io::Result<Vec<ScheduledPacket>> {
    let metadata = ParsedPacket::parse(&packet);
    let payload = metadata
        .as_ref()
        .map_or(&[][..], |parsed| &packet[parsed.payload_range.clone()]);
    let context = PacketContext {
        elapsed_millis: started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
        direction,
        ip_version: metadata
            .as_ref()
            .map_or(IpVersion::V4, |value| value.ip_version),
        transport: metadata
            .as_ref()
            .map_or(TransportProtocol::Other, |value| value.transport),
        destination_port: metadata.as_ref().and_then(|value| value.destination_port),
        remote_address: metadata
            .as_ref()
            .map(|value| value.remote_address(direction)),
        remote_port: metadata
            .as_ref()
            .and_then(|value| value.remote_port(direction)),
        tcp_flags: metadata
            .as_ref()
            .map_or_else(BTreeSet::new, |value| value.tcp_flags.clone()),
        packet_len: packet.len(),
        payload,
    };
    let (decision, _engine_error) = engine.evaluate(&context);
    if decision.drop_reason.is_some() || decision.copies == 0 {
        RUNTIME_STATS
            .impairment_packets_dropped
            .fetch_add(1, AtomicOrdering::Relaxed);
        return Ok(Vec::new());
    }

    record_non_terminal_impairments(&decision, payload);
    if metadata.is_none() && !matches!(decision.path_mtu_action, PathMtuAction::None) {
        return Err(path_mtu_error(
            decision.path_mtu_action,
            "触发包不是可解析的 IPv4/IPv6 数据报",
        ));
    }

    if let Some(metadata) = metadata {
        let mut packet_changed = apply_payload_change(&mut packet, &metadata, &decision.payload);
        if matches!(decision.path_mtu_action, PathMtuAction::ClampMss(_)) {
            let clamped = clamp_existing_tcp_mss(&mut packet, &metadata, decision.path_mtu_action);
            if clamped {
                RUNTIME_STATS
                    .impairment_mss_clamps
                    .fetch_add(1, AtomicOrdering::Relaxed);
            }
            packet_changed |= clamped;
        } else if let Some(pmtu_packets) = prepare_path_mtu_packets(&packet, &decision, sequence)? {
            return Ok(pmtu_packets);
        }
        // 未修改的 IP 包必须保持字节级透传，避免改变内核认可的 checksum 表示方式。
        if packet_changed {
            refresh_checksums(&mut packet, &metadata);
        }
    }

    let delay_millis = decision
        .delay_millis
        .saturating_add(decision.reorder_hold_millis);
    Ok(vec![ScheduledPacket {
        due: TokioInstant::now() + Duration::from_millis(delay_millis),
        sequence,
        copies: decision.copies,
        route: PacketRoute::Forward,
        packet,
    }])
}

fn record_non_terminal_impairments(decision: &crate::PacketDecision, original_payload: &[u8]) {
    if decision.copies > 1 {
        RUNTIME_STATS
            .impairment_packets_duplicated
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
    if decision.reorder_hold_millis > 0 {
        RUNTIME_STATS
            .impairment_packets_reordered
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
    if decision.payload.as_slice() != original_payload {
        RUNTIME_STATS
            .impairment_packets_corrupted
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
    RUNTIME_STATS.impairment_delay_millis_total.fetch_add(
        decision
            .delay_millis
            .saturating_add(decision.reorder_hold_millis),
        AtomicOrdering::Relaxed,
    );
}

fn apply_payload_change(packet: &mut [u8], metadata: &ParsedPacket, payload: &[u8]) -> bool {
    if payload.len() != metadata.payload_range.len()
        || payload == &packet[metadata.payload_range.clone()]
    {
        return false;
    }
    packet[metadata.payload_range.clone()].copy_from_slice(payload);
    true
}

pub(super) fn prepare_path_mtu_packets(
    packet: &[u8],
    decision: &crate::PacketDecision,
    sequence: u64,
) -> io::Result<Option<Vec<ScheduledPacket>>> {
    let delay_millis = decision
        .delay_millis
        .saturating_add(decision.reorder_hold_millis);
    match decision.path_mtu_action {
        PathMtuAction::FragmentIpv4(mtu) => fragment_ipv4_packet(packet, mtu).map(|fragments| {
            RUNTIME_STATS
                .impairment_pmtu_fragments
                .fetch_add(fragments.len() as u64, AtomicOrdering::Relaxed);
            fragments
                .into_iter()
                .enumerate()
                .map(|(index, packet)| ScheduledPacket {
                    due: TokioInstant::now() + Duration::from_millis(delay_millis),
                    sequence: sequence.saturating_add(index as u64),
                    copies: decision.copies,
                    route: PacketRoute::Forward,
                    packet,
                })
                .collect()
        }),
        PathMtuAction::Icmpv4FragmentationNeeded(mtu) => {
            build_icmpv4_fragmentation_needed(packet, mtu).map(|signal| {
                record_pmtu_signal();
                vec![reverse_signal(sequence, signal)]
            })
        }
        PathMtuAction::Icmpv6PacketTooBig(mtu) => {
            build_icmpv6_packet_too_big(packet, mtu).map(|signal| {
                record_pmtu_signal();
                vec![reverse_signal(sequence, signal)]
            })
        }
        PathMtuAction::None | PathMtuAction::ClampMss(_) => return Ok(None),
    }
    .map(Some)
    .ok_or_else(|| path_mtu_error(decision.path_mtu_action, "无法构造所需分片或 ICMP 信号"))
}

fn path_mtu_error(action: PathMtuAction, reason: &str) -> io::Error {
    RUNTIME_STATS
        .impairment_unimplemented_pmtu_actions
        .fetch_add(1, AtomicOrdering::Relaxed);
    let error = format!(
        "无法为当前 IP 包执行路径 MTU 动作 {action:?}：{reason}；已终止数据面并恢复系统直连"
    );
    record_runtime_error(&error);
    io::Error::other(error)
}

fn record_pmtu_signal() {
    RUNTIME_STATS
        .impairment_packets_dropped
        .fetch_add(1, AtomicOrdering::Relaxed);
    RUNTIME_STATS
        .impairment_pmtu_signals
        .fetch_add(1, AtomicOrdering::Relaxed);
}

fn reverse_signal(sequence: u64, packet: Vec<u8>) -> ScheduledPacket {
    ScheduledPacket {
        due: TokioInstant::now(),
        sequence,
        copies: 1,
        route: PacketRoute::Reverse,
        packet,
    }
}

pub(super) async fn run_packet_scheduler<F, Fut>(
    mut receiver: mpsc::Receiver<ScheduledPacket>,
    cancellation: CancellationToken,
    send: F,
) -> io::Result<()>
where
    F: Fn(PacketRoute, Vec<u8>) -> Fut,
    Fut: Future<Output = io::Result<()>>,
{
    let mut queue = BinaryHeap::new();
    let mut input_closed = false;
    loop {
        if queue.is_empty() {
            if input_closed {
                return Ok(());
            }
            tokio::select! {
                () = cancellation.cancelled() => return Ok(()),
                packet = receiver.recv() => match packet {
                    Some(packet) => queue.push(packet),
                    None => input_closed = true,
                },
            }
            continue;
        }

        let due = queue.peek().expect("队列非空").due;
        tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            packet = receiver.recv(), if !input_closed => match packet {
                Some(packet) => queue.push(packet),
                None => input_closed = true,
            },
            () = sleep_until(due) => {
                let packet = queue.pop().expect("到期队列非空");
                for _ in 0..packet.copies {
                    send(packet.route, packet.packet.clone()).await?;
                }
            }
        }
    }
}
