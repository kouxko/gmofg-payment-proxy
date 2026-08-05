use std::{
    io::{self, Read, Write},
    sync::{Arc, atomic::Ordering as AtomicOrdering},
    time::Instant,
};

use tokio::{net::UnixDatagram, sync::mpsc};
use tun2proxy::CancellationToken;

use crate::{Direction, FailOpenEngine, IpVersion, TcpFlag};

use super::{
    MAX_IP_PACKET_SIZE,
    packet::{ParsedPacket, checksum, transport_checksum_is_valid},
    scheduler::{PacketRoute, prepare_scheduled_packets, run_packet_scheduler},
    stats::RUNTIME_STATS,
    tun::ManagedTunFile,
};

pub(super) async fn pump_tun_to_proxy(
    tun: Arc<tokio::io::unix::AsyncFd<ManagedTunFile>>,
    proxy: Arc<UnixDatagram>,
    engine: Arc<FailOpenEngine>,
    started_at: Instant,
    cancellation: CancellationToken,
) -> io::Result<()> {
    let (sender, receiver) = mpsc::channel(512);
    let reader_cancellation = cancellation.child_token();
    let reader_tun = tun.clone();
    let reader = async move {
        let mut packet = vec![0_u8; MAX_IP_PACKET_SIZE];
        let mut sequence = 0_u64;
        loop {
            let size = tokio::select! {
                () = reader_cancellation.cancelled() => return Ok(()),
                size = read_tun_packet(&reader_tun, &mut packet) => size?,
            };
            RUNTIME_STATS
                .tun_upload_packets
                .fetch_add(1, AtomicOrdering::Relaxed);
            RUNTIME_STATS
                .tun_upload_bytes
                .fetch_add(size as u64, AtomicOrdering::Relaxed);
            record_upload_packet_diagnostics(&packet[..size]);
            for scheduled in prepare_scheduled_packets(
                packet[..size].to_vec(),
                Direction::Upload,
                &engine,
                started_at,
                sequence,
            )? {
                sequence = sequence.saturating_add(1);
                if sender.send(scheduled).await.is_err() {
                    return Ok(());
                }
            }
        }
    };
    let scheduler = run_packet_scheduler(
        receiver,
        cancellation.child_token(),
        move |route, packet| {
            let proxy = proxy.clone();
            let tun = tun.clone();
            async move {
                match route {
                    PacketRoute::Forward => proxy.send(&packet).await.map(|_| {
                        RUNTIME_STATS
                            .proxy_upload_packets
                            .fetch_add(1, AtomicOrdering::Relaxed);
                    }),
                    PacketRoute::Reverse => {
                        let size = packet.len() as u64;
                        write_tun_packet(&tun, &packet).await?;
                        RUNTIME_STATS
                            .tun_download_packets
                            .fetch_add(1, AtomicOrdering::Relaxed);
                        RUNTIME_STATS
                            .tun_download_bytes
                            .fetch_add(size, AtomicOrdering::Relaxed);
                        Ok(())
                    }
                }
            }
        },
    );
    tokio::select! {
        () = cancellation.cancelled() => Ok(()),
        result = reader => result,
        result = scheduler => result,
    }
}

pub(super) async fn pump_proxy_to_tun(
    proxy: Arc<UnixDatagram>,
    tun: Arc<tokio::io::unix::AsyncFd<ManagedTunFile>>,
    engine: Arc<FailOpenEngine>,
    started_at: Instant,
    cancellation: CancellationToken,
) -> io::Result<()> {
    let (sender, receiver) = mpsc::channel(512);
    let reader_cancellation = cancellation.child_token();
    let reader_proxy = proxy.clone();
    let reader = async move {
        let mut packet = vec![0_u8; MAX_IP_PACKET_SIZE];
        let mut sequence = 0_u64;
        loop {
            let size = tokio::select! {
                () = reader_cancellation.cancelled() => return Ok(()),
                result = reader_proxy.recv(&mut packet) => result?,
            };
            RUNTIME_STATS
                .proxy_download_packets
                .fetch_add(1, AtomicOrdering::Relaxed);
            record_download_packet_diagnostics(&packet[..size]);
            for scheduled in prepare_scheduled_packets(
                packet[..size].to_vec(),
                Direction::Download,
                &engine,
                started_at,
                sequence,
            )? {
                sequence = sequence.saturating_add(1);
                if sender.send(scheduled).await.is_err() {
                    return Ok(());
                }
            }
        }
    };
    let scheduler = run_packet_scheduler(
        receiver,
        cancellation.child_token(),
        move |route, packet| {
            let tun = tun.clone();
            let proxy = proxy.clone();
            async move {
                match route {
                    PacketRoute::Forward => {
                        let size = packet.len() as u64;
                        write_tun_packet(&tun, &packet).await?;
                        RUNTIME_STATS
                            .tun_download_packets
                            .fetch_add(1, AtomicOrdering::Relaxed);
                        RUNTIME_STATS
                            .tun_download_bytes
                            .fetch_add(size, AtomicOrdering::Relaxed);
                        Ok(())
                    }
                    PacketRoute::Reverse => proxy.send(&packet).await.map(|_| {
                        RUNTIME_STATS
                            .proxy_upload_packets
                            .fetch_add(1, AtomicOrdering::Relaxed);
                    }),
                }
            }
        },
    );
    tokio::select! {
        () = cancellation.cancelled() => Ok(()),
        result = reader => result,
        result = scheduler => result,
    }
}

async fn read_tun_packet(
    tun: &tokio::io::unix::AsyncFd<ManagedTunFile>,
    buffer: &mut [u8],
) -> io::Result<usize> {
    loop {
        let mut guard = tun.readable().await?;
        match guard.try_io(|inner| inner.get_ref().read(buffer)) {
            Ok(result) => return result,
            Err(_would_block) => {}
        }
    }
}

async fn write_tun_packet(
    tun: &tokio::io::unix::AsyncFd<ManagedTunFile>,
    packet: &[u8],
) -> io::Result<()> {
    let mut written = 0;
    while written < packet.len() {
        let mut guard = tun.writable().await?;
        match guard.try_io(|inner| inner.get_ref().write(&packet[written..])) {
            Ok(Ok(size)) => written += size,
            Ok(Err(error)) => return Err(error),
            Err(_would_block) => {}
        }
    }
    Ok(())
}

fn record_upload_packet_diagnostics(packet: &[u8]) {
    let Some(parsed) = ParsedPacket::parse(packet) else {
        return;
    };
    if parsed.tcp_flags.contains(&TcpFlag::Syn) {
        RUNTIME_STATS
            .upload_tcp_syn_packets
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
    if parsed.tcp_flags.contains(&TcpFlag::Ack) {
        RUNTIME_STATS
            .upload_tcp_ack_packets
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
}

fn record_download_packet_diagnostics(packet: &[u8]) {
    let Some(parsed) = ParsedPacket::parse(packet) else {
        return;
    };
    if parsed.tcp_flags.contains(&TcpFlag::SynAck) {
        RUNTIME_STATS
            .download_tcp_syn_ack_packets
            .fetch_add(1, AtomicOrdering::Relaxed);
    }

    let declared_length = match parsed.ip_version {
        IpVersion::V4 if packet.len() >= 20 => {
            usize::from(u16::from_be_bytes([packet[2], packet[3]]))
        }
        IpVersion::V6 if packet.len() >= 40 => {
            40 + usize::from(u16::from_be_bytes([packet[4], packet[5]]))
        }
        _ => packet.len(),
    };
    if declared_length != packet.len() {
        RUNTIME_STATS
            .download_ip_length_mismatches
            .fetch_add(1, AtomicOrdering::Relaxed);
    }

    if parsed.ip_version == IpVersion::V4 {
        let header_length = usize::from(packet[0] & 0x0f) * 4;
        if header_length > packet.len() || checksum(&packet[..header_length]) != 0 {
            RUNTIME_STATS
                .download_ip_checksum_failures
                .fetch_add(1, AtomicOrdering::Relaxed);
        }
    }

    if !transport_checksum_is_valid(packet, &parsed) {
        RUNTIME_STATS
            .download_transport_checksum_failures
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
}
