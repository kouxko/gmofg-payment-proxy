use std::{
    collections::BTreeMap,
    net::IpAddr,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Mutex,
};

use thiserror::Error;

use crate::{
    Direction, DropReason, EngineStats, IpVersion, PacketContext, PacketDecision, PathMtuAction,
    PmtuMode, TcpFlag, TransportProtocol, ValidatedProfile, WeakNetworkProfile,
    rng::DeterministicRng, validation::parse_ip_cidr,
};

/// 引擎的可恢复错误。调用方必须使用 fail-open 包装器，不能因为观测工具故障阻断业务。
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum EngineError {
    #[error("弱网引擎互斥锁已损坏")]
    Poisoned,
    #[error("弱网引擎发生未预期 panic")]
    Panicked,
}

#[derive(Clone, Copy, Debug, Default)]
struct DirectionState {
    next_rate_slot_millis: u64,
    burst_bad: bool,
}

#[derive(Clone, Debug)]
struct CompiledDestinationTarget {
    network: IpAddr,
    prefix: u8,
    ports: Vec<u16>,
}

impl CompiledDestinationTarget {
    fn matches(&self, address: IpAddr, port: Option<u16>) -> bool {
        if !ip_is_in_network(address, self.network, self.prefix) {
            return false;
        }
        self.ports.is_empty() || port.is_some_and(|port| self.ports.contains(&port))
    }
}

/// 对包序列实施确定性故障的纯 Rust 引擎。
#[derive(Debug)]
pub struct ImpairmentEngine {
    profile: WeakNetworkProfile,
    destination_targets: Vec<CompiledDestinationTarget>,
    rng: DeterministicRng,
    upload: DirectionState,
    download: DirectionState,
    tcp_flag_counts: BTreeMap<(Direction, TcpFlag), u64>,
    stats: EngineStats,
}

impl ImpairmentEngine {
    #[must_use]
    pub fn new(profile: &ValidatedProfile) -> Self {
        let weak = profile.as_profile().weak_network.clone();
        let destination_targets = profile
            .as_profile()
            .destination_targets
            .iter()
            .filter_map(|target| {
                parse_ip_cidr(&target.cidr).map(|(network, prefix)| CompiledDestinationTarget {
                    network,
                    prefix,
                    ports: target.ports.clone(),
                })
            })
            .collect();
        Self {
            rng: DeterministicRng::new(weak.seed),
            profile: weak,
            destination_targets,
            upload: DirectionState::default(),
            download: DirectionState::default(),
            tcp_flag_counts: BTreeMap::new(),
            stats: EngineStats::default(),
        }
    }

    /// 评估一个包。所有时间均使用相对于引擎启动时刻的单调毫秒值。
    pub fn evaluate(&mut self, packet: &PacketContext<'_>) -> PacketDecision {
        self.stats.packets_seen = self.stats.packets_seen.saturating_add(1);
        self.stats.bytes_seen = self
            .stats
            .bytes_seen
            .saturating_add(packet.packet_len as u64);

        let mut decision = PacketDecision::pass(packet.payload);
        if !self.matches_destination(packet) {
            self.finish_forward(packet, &decision);
            return decision;
        }
        decision.path_mtu_action = self.path_mtu_action(packet);

        if self.is_blackout(packet.elapsed_millis) {
            return self.finish_drop(decision, DropReason::Blackout);
        }
        if self.profile.dns_blackhole
            && matches!(
                packet.transport,
                TransportProtocol::Tcp | TransportProtocol::Udp
            )
            && matches!(packet.destination_port, Some(53 | 853))
        {
            return self.finish_drop(decision, DropReason::DnsBlackhole);
        }
        if self.should_drop_nth_tcp_flag(packet) {
            return self.finish_drop(decision, DropReason::NthTcpFlag);
        }
        if self.path_mtu_blackholes(packet) {
            return self.finish_drop(decision, DropReason::PmtuBlackhole);
        }
        if self
            .rng
            .hits_basis_points(self.profile.random_loss_basis_points)
        {
            return self.finish_drop(decision, DropReason::RandomLoss);
        }
        if self.should_burst_drop(packet.direction) {
            return self.finish_drop(decision, DropReason::BurstLoss);
        }

        decision.delay_millis = self.delay_with_jitter();
        decision.delay_millis = decision
            .delay_millis
            .saturating_add(self.rate_limit_delay(packet));

        if self
            .rng
            .hits_basis_points(self.profile.duplicate_basis_points)
        {
            decision.copies = 2;
            self.stats.duplicated_packets = self.stats.duplicated_packets.saturating_add(1);
        }
        if self
            .rng
            .hits_basis_points(self.profile.reorder_basis_points)
        {
            decision.reorder_hold_millis =
                self.rng.inclusive(self.profile.maximum_reorder_hold_millis);
            self.stats.reordered_packets = self.stats.reordered_packets.saturating_add(1);
        }
        if !decision.payload.is_empty()
            && self
                .rng
                .hits_basis_points(self.profile.corruption.probability_basis_points)
        {
            self.corrupt_payload(&mut decision.payload);
            self.stats.corrupted_packets = self.stats.corrupted_packets.saturating_add(1);
        }

        self.finish_forward(packet, &decision);
        decision
    }

    #[must_use]
    pub const fn stats(&self) -> EngineStats {
        self.stats
    }

    fn finish_drop(&mut self, mut decision: PacketDecision, reason: DropReason) -> PacketDecision {
        decision.drop_reason = Some(reason);
        decision.copies = 0;
        self.stats.packets_dropped = self.stats.packets_dropped.saturating_add(1);
        decision
    }

    fn matches_destination(&self, packet: &PacketContext<'_>) -> bool {
        if self.destination_targets.is_empty() {
            return true;
        }
        packet.remote_address.is_some_and(|address| {
            self.destination_targets
                .iter()
                .any(|target| target.matches(address, packet.remote_port))
        })
    }

    fn finish_forward(&mut self, packet: &PacketContext<'_>, decision: &PacketDecision) {
        self.stats.packets_forwarded = self.stats.packets_forwarded.saturating_add(1);
        self.stats.bytes_forwarded = self
            .stats
            .bytes_forwarded
            .saturating_add(packet.packet_len as u64 * u64::from(decision.copies));
    }

    fn is_blackout(&self, elapsed_millis: u64) -> bool {
        self.profile.blackout_windows.iter().any(|window| {
            elapsed_millis >= window.start_after_millis
                && elapsed_millis
                    < window
                        .start_after_millis
                        .saturating_add(window.duration_millis)
        })
    }

    fn should_drop_nth_tcp_flag(&mut self, packet: &PacketContext<'_>) -> bool {
        if packet.transport != TransportProtocol::Tcp {
            return false;
        }
        let mut drop = false;
        for flag in &packet.tcp_flags {
            let count = self
                .tcp_flag_counts
                .entry((packet.direction, *flag))
                .or_default();
            *count = count.saturating_add(1);
            drop |= self.profile.nth_tcp_flag_drops.iter().any(|configured| {
                configured.direction == packet.direction
                    && configured.flag == *flag
                    && configured.nth == *count
            });
        }
        drop
    }

    fn should_burst_drop(&mut self, direction: Direction) -> bool {
        let Some(burst) = self.profile.burst_loss else {
            return false;
        };
        let state = match direction {
            Direction::Upload => &mut self.upload,
            Direction::Download => &mut self.download,
        };
        if state.burst_bad {
            if self
                .rng
                .hits_basis_points(burst.leave_bad_state_basis_points)
            {
                state.burst_bad = false;
            }
        } else if self
            .rng
            .hits_basis_points(burst.enter_bad_state_basis_points)
        {
            state.burst_bad = true;
        }
        let loss = if state.burst_bad {
            burst.bad_state_loss_basis_points
        } else {
            burst.good_state_loss_basis_points
        };
        self.rng.hits_basis_points(loss)
    }

    fn delay_with_jitter(&mut self) -> u64 {
        let jitter = self.profile.uniform_jitter_millis;
        if jitter == 0 {
            return self.profile.fixed_delay_millis;
        }
        let magnitude = self.rng.inclusive(jitter);
        if self.rng.next_u64() & 1 == 0 {
            self.profile.fixed_delay_millis.saturating_sub(magnitude)
        } else {
            self.profile.fixed_delay_millis.saturating_add(magnitude)
        }
    }

    fn rate_limit_delay(&mut self, packet: &PacketContext<'_>) -> u64 {
        let bytes_per_second = match packet.direction {
            Direction::Upload => self.profile.upload_bytes_per_second,
            Direction::Download => self.profile.download_bytes_per_second,
        };
        let Some(bytes_per_second) = bytes_per_second else {
            return 0;
        };
        let state = match packet.direction {
            Direction::Upload => &mut self.upload,
            Direction::Download => &mut self.download,
        };
        let slot = state.next_rate_slot_millis.max(packet.elapsed_millis);
        let wait = slot.saturating_sub(packet.elapsed_millis);
        let transfer_millis = (packet.packet_len as u64)
            .saturating_mul(1_000)
            .saturating_add(bytes_per_second.saturating_sub(1))
            / bytes_per_second;
        state.next_rate_slot_millis = slot.saturating_add(transfer_millis);
        wait
    }

    fn path_mtu_blackholes(&self, packet: &PacketContext<'_>) -> bool {
        self.profile.path_mtu.mode == PmtuMode::Blackhole
            && self
                .profile
                .path_mtu
                .mtu
                .is_some_and(|mtu| packet.packet_len > usize::from(mtu))
    }

    fn path_mtu_action(&self, packet: &PacketContext<'_>) -> PathMtuAction {
        if packet.transport == TransportProtocol::Tcp
            && packet.tcp_flags.contains(&TcpFlag::Syn)
            && let Some(mss) = self.profile.path_mtu.mss_clamp
        {
            return PathMtuAction::ClampMss(mss);
        }
        let Some(mtu) = self.profile.path_mtu.mtu else {
            return PathMtuAction::None;
        };
        if packet.packet_len <= usize::from(mtu) {
            return PathMtuAction::None;
        }
        match (self.profile.path_mtu.mode, packet.ip_version) {
            (PmtuMode::FragmentOrPacketTooBig, IpVersion::V4) => PathMtuAction::FragmentIpv4(mtu),
            (PmtuMode::SignalTooBig, IpVersion::V4) => {
                PathMtuAction::Icmpv4FragmentationNeeded(mtu)
            }
            (PmtuMode::FragmentOrPacketTooBig | PmtuMode::SignalTooBig, IpVersion::V6) => {
                PathMtuAction::Icmpv6PacketTooBig(mtu)
            }
            (PmtuMode::Pass | PmtuMode::Blackhole, _) => PathMtuAction::None,
        }
    }

    fn corrupt_payload(&mut self, payload: &mut [u8]) {
        let bit_count = self.profile.corruption.bits_per_packet;
        for _ in 0..bit_count {
            let bit = self.rng.next_u64() % (payload.len() as u64 * 8);
            let byte_index = (bit / 8) as usize;
            payload[byte_index] ^= 1 << (bit % 8);
        }
    }
}

fn ip_is_in_network(address: IpAddr, network: IpAddr, prefix: u8) -> bool {
    match (address, network) {
        (IpAddr::V4(address), IpAddr::V4(network)) => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - u32::from(prefix))
            };
            u32::from(address) & mask == u32::from(network) & mask
        }
        (IpAddr::V6(address), IpAddr::V6(network)) => {
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - u32::from(prefix))
            };
            u128::from(address) & mask == u128::from(network) & mask
        }
        _ => false,
    }
}

/// 线程安全、永不让弱网观测故障阻塞真实网络的包装器。
#[derive(Debug)]
pub struct FailOpenEngine {
    inner: Mutex<ImpairmentEngine>,
}

impl FailOpenEngine {
    #[must_use]
    pub fn new(profile: &ValidatedProfile) -> Self {
        Self {
            inner: Mutex::new(ImpairmentEngine::new(profile)),
        }
    }

    /// 引擎出错或 panic 时返回原样放行决策，同时把错误交给上层记录。
    pub fn evaluate(&self, packet: &PacketContext<'_>) -> (PacketDecision, Option<EngineError>) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut engine = self.inner.lock().map_err(|_| EngineError::Poisoned)?;
            Ok::<_, EngineError>(engine.evaluate(packet))
        }));
        match result {
            Ok(Ok(decision)) => (decision, None),
            Ok(Err(error)) => (PacketDecision::pass(packet.payload), Some(error)),
            Err(_) => (
                PacketDecision::pass(packet.payload),
                Some(EngineError::Panicked),
            ),
        }
    }

    pub fn stats(&self) -> Result<EngineStats, EngineError> {
        self.inner
            .lock()
            .map(|engine| engine.stats())
            .map_err(|_| EngineError::Poisoned)
    }
}

#[cfg(test)]
#[path = "engine/tests.rs"]
mod tests;
