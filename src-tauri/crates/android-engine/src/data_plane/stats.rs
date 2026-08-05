//! 数据面运行周期的无 Payload 统计与错误快照。

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicU64, Ordering as AtomicOrdering},
};

/// 设备端控制页和自动化验收共用的最小数据面计数器。
///
/// 这些值只记录包数/字节数和连接阶段，不保存任何应用报文内容。没有这些计数器时，
/// “VPN 已连接但目标应用没有网络”无法区分是 Android 没把包送入 TUN、弱网调度没有
/// 转发、`tun2proxy` 没有建连，还是受保护的外连失败。
#[derive(Debug, Default)]
pub(super) struct RuntimeStats {
    pub(super) tun_upload_packets: AtomicU64,
    pub(super) tun_upload_bytes: AtomicU64,
    pub(super) proxy_upload_packets: AtomicU64,
    pub(super) proxy_download_packets: AtomicU64,
    pub(super) tun_download_packets: AtomicU64,
    pub(super) tun_download_bytes: AtomicU64,
    pub(super) socks_clients: AtomicU64,
    pub(super) socks_connect_attempts: AtomicU64,
    pub(super) socks_connect_successes: AtomicU64,
    pub(super) protect_failures: AtomicU64,
    pub(super) upload_tcp_syn_packets: AtomicU64,
    pub(super) upload_tcp_ack_packets: AtomicU64,
    pub(super) download_tcp_syn_ack_packets: AtomicU64,
    pub(super) download_ip_length_mismatches: AtomicU64,
    pub(super) download_ip_checksum_failures: AtomicU64,
    pub(super) download_transport_checksum_failures: AtomicU64,
    pub(super) impairment_packets_dropped: AtomicU64,
    pub(super) impairment_packets_duplicated: AtomicU64,
    pub(super) impairment_packets_reordered: AtomicU64,
    pub(super) impairment_packets_corrupted: AtomicU64,
    pub(super) impairment_delay_millis_total: AtomicU64,
    pub(super) impairment_mss_clamps: AtomicU64,
    pub(super) impairment_pmtu_fragments: AtomicU64,
    pub(super) impairment_pmtu_signals: AtomicU64,
    pub(super) impairment_unimplemented_pmtu_actions: AtomicU64,
}

pub(super) static RUNTIME_STATS: RuntimeStats = RuntimeStats {
    tun_upload_packets: AtomicU64::new(0),
    tun_upload_bytes: AtomicU64::new(0),
    proxy_upload_packets: AtomicU64::new(0),
    proxy_download_packets: AtomicU64::new(0),
    tun_download_packets: AtomicU64::new(0),
    tun_download_bytes: AtomicU64::new(0),
    socks_clients: AtomicU64::new(0),
    socks_connect_attempts: AtomicU64::new(0),
    socks_connect_successes: AtomicU64::new(0),
    protect_failures: AtomicU64::new(0),
    upload_tcp_syn_packets: AtomicU64::new(0),
    upload_tcp_ack_packets: AtomicU64::new(0),
    download_tcp_syn_ack_packets: AtomicU64::new(0),
    download_ip_length_mismatches: AtomicU64::new(0),
    download_ip_checksum_failures: AtomicU64::new(0),
    download_transport_checksum_failures: AtomicU64::new(0),
    impairment_packets_dropped: AtomicU64::new(0),
    impairment_packets_duplicated: AtomicU64::new(0),
    impairment_packets_reordered: AtomicU64::new(0),
    impairment_packets_corrupted: AtomicU64::new(0),
    impairment_delay_millis_total: AtomicU64::new(0),
    impairment_mss_clamps: AtomicU64::new(0),
    impairment_pmtu_fragments: AtomicU64::new(0),
    impairment_pmtu_signals: AtomicU64::new(0),
    impairment_unimplemented_pmtu_actions: AtomicU64::new(0),
};

static LAST_RUNTIME_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
// 每次启动数据面都会获得一个新的运行周期。SOCKS 会话是独立 Tokio 任务，旧周期
// 停止时这些任务可能仍在短暂收尾；没有周期校验时，它们会把旧连接错误写进新周期。
static ACTIVE_RUNTIME_EPOCH: AtomicU64 = AtomicU64::new(0);

pub(super) fn record_runtime_error(error: &str) {
    if let Ok(mut slot) = LAST_RUNTIME_ERROR.get_or_init(|| Mutex::new(None)).lock() {
        *slot = Some(error.to_owned());
    }
}

pub(super) fn record_runtime_error_for_epoch(epoch: u64, error: &str) {
    if ACTIVE_RUNTIME_EPOCH.load(AtomicOrdering::Acquire) == epoch {
        record_runtime_error(error);
    }
}

fn increment_for_epoch(epoch: u64, counter: &AtomicU64) {
    if ACTIVE_RUNTIME_EPOCH.load(AtomicOrdering::Acquire) == epoch {
        counter.fetch_add(1, AtomicOrdering::Relaxed);
    }
}

pub(crate) fn record_socks_client(epoch: u64) {
    increment_for_epoch(epoch, &RUNTIME_STATS.socks_clients);
}

pub(crate) fn record_socks_connect_attempt(epoch: u64) {
    increment_for_epoch(epoch, &RUNTIME_STATS.socks_connect_attempts);
}

pub(crate) fn record_socks_connect_success(epoch: u64) {
    increment_for_epoch(epoch, &RUNTIME_STATS.socks_connect_successes);
}

pub(crate) fn record_socket_protection_failure(epoch: u64) {
    increment_for_epoch(epoch, &RUNTIME_STATS.protect_failures);
}

pub(crate) fn reset_runtime_stats() -> u64 {
    let epoch = ACTIVE_RUNTIME_EPOCH.fetch_add(1, AtomicOrdering::AcqRel) + 1;
    for value in [
        &RUNTIME_STATS.tun_upload_packets,
        &RUNTIME_STATS.tun_upload_bytes,
        &RUNTIME_STATS.proxy_upload_packets,
        &RUNTIME_STATS.proxy_download_packets,
        &RUNTIME_STATS.tun_download_packets,
        &RUNTIME_STATS.tun_download_bytes,
        &RUNTIME_STATS.socks_clients,
        &RUNTIME_STATS.socks_connect_attempts,
        &RUNTIME_STATS.socks_connect_successes,
        &RUNTIME_STATS.protect_failures,
        &RUNTIME_STATS.upload_tcp_syn_packets,
        &RUNTIME_STATS.upload_tcp_ack_packets,
        &RUNTIME_STATS.download_tcp_syn_ack_packets,
        &RUNTIME_STATS.download_ip_length_mismatches,
        &RUNTIME_STATS.download_ip_checksum_failures,
        &RUNTIME_STATS.download_transport_checksum_failures,
        &RUNTIME_STATS.impairment_packets_dropped,
        &RUNTIME_STATS.impairment_packets_duplicated,
        &RUNTIME_STATS.impairment_packets_reordered,
        &RUNTIME_STATS.impairment_packets_corrupted,
        &RUNTIME_STATS.impairment_delay_millis_total,
        &RUNTIME_STATS.impairment_mss_clamps,
        &RUNTIME_STATS.impairment_pmtu_fragments,
        &RUNTIME_STATS.impairment_pmtu_signals,
        &RUNTIME_STATS.impairment_unimplemented_pmtu_actions,
    ] {
        value.store(0, AtomicOrdering::Relaxed);
    }
    if let Ok(mut slot) = LAST_RUNTIME_ERROR.get_or_init(|| Mutex::new(None)).lock() {
        *slot = None;
    }
    epoch
}

pub(crate) fn runtime_stats_json() -> String {
    let last_error = LAST_RUNTIME_ERROR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|value| value.clone());
    serde_json::json!({
        "tun_upload_packets": RUNTIME_STATS.tun_upload_packets.load(AtomicOrdering::Relaxed),
        "tun_upload_bytes": RUNTIME_STATS.tun_upload_bytes.load(AtomicOrdering::Relaxed),
        "proxy_upload_packets": RUNTIME_STATS.proxy_upload_packets.load(AtomicOrdering::Relaxed),
        "proxy_download_packets": RUNTIME_STATS.proxy_download_packets.load(AtomicOrdering::Relaxed),
        "tun_download_packets": RUNTIME_STATS.tun_download_packets.load(AtomicOrdering::Relaxed),
        "tun_download_bytes": RUNTIME_STATS.tun_download_bytes.load(AtomicOrdering::Relaxed),
        "socks_clients": RUNTIME_STATS.socks_clients.load(AtomicOrdering::Relaxed),
        "socks_connect_attempts": RUNTIME_STATS.socks_connect_attempts.load(AtomicOrdering::Relaxed),
        "socks_connect_successes": RUNTIME_STATS.socks_connect_successes.load(AtomicOrdering::Relaxed),
        "protect_failures": RUNTIME_STATS.protect_failures.load(AtomicOrdering::Relaxed),
        "upload_tcp_syn_packets": RUNTIME_STATS.upload_tcp_syn_packets.load(AtomicOrdering::Relaxed),
        "upload_tcp_ack_packets": RUNTIME_STATS.upload_tcp_ack_packets.load(AtomicOrdering::Relaxed),
        "download_tcp_syn_ack_packets": RUNTIME_STATS.download_tcp_syn_ack_packets.load(AtomicOrdering::Relaxed),
        "download_ip_length_mismatches": RUNTIME_STATS.download_ip_length_mismatches.load(AtomicOrdering::Relaxed),
        "download_ip_checksum_failures": RUNTIME_STATS.download_ip_checksum_failures.load(AtomicOrdering::Relaxed),
        "download_transport_checksum_failures": RUNTIME_STATS.download_transport_checksum_failures.load(AtomicOrdering::Relaxed),
        "impairment_packets_dropped": RUNTIME_STATS.impairment_packets_dropped.load(AtomicOrdering::Relaxed),
        "impairment_packets_duplicated": RUNTIME_STATS.impairment_packets_duplicated.load(AtomicOrdering::Relaxed),
        "impairment_packets_reordered": RUNTIME_STATS.impairment_packets_reordered.load(AtomicOrdering::Relaxed),
        "impairment_packets_corrupted": RUNTIME_STATS.impairment_packets_corrupted.load(AtomicOrdering::Relaxed),
        "impairment_delay_millis_total": RUNTIME_STATS.impairment_delay_millis_total.load(AtomicOrdering::Relaxed),
        "impairment_mss_clamps": RUNTIME_STATS.impairment_mss_clamps.load(AtomicOrdering::Relaxed),
        "impairment_pmtu_fragments": RUNTIME_STATS.impairment_pmtu_fragments.load(AtomicOrdering::Relaxed),
        "impairment_pmtu_signals": RUNTIME_STATS.impairment_pmtu_signals.load(AtomicOrdering::Relaxed),
        "impairment_unimplemented_pmtu_actions": RUNTIME_STATS.impairment_unimplemented_pmtu_actions.load(AtomicOrdering::Relaxed),
        "last_error": last_error,
    })
    .to_string()
}
