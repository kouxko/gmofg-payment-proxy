use std::{
    io::{self, Read},
    net::{IpAddr, Ipv6Addr},
    sync::mpsc as sync_mpsc,
    thread,
    time::{Duration, Instant},
};

use tokio::{sync::mpsc, time::Instant as TokioInstant};
use tun2proxy::CancellationToken;

use crate::{Direction, PathMtuAction, TcpFlag};

use super::{
    DataPlaneHandle, ThreadFinishedNotifier,
    packet::{
        ParsedPacket, build_icmpv4_fragmentation_needed, build_icmpv6_packet_too_big, checksum,
        fragment_ipv4_packet,
    },
    scheduler::{PacketRoute, ScheduledPacket, prepare_path_mtu_packets, run_packet_scheduler},
    tun::ManagedTunFile,
};

#[test]
fn parses_ipv4_tcp_metadata() {
    let mut packet = vec![0_u8; 40];
    packet[0] = 0x45;
    packet[9] = 6;
    packet[12..16].copy_from_slice(&[10, 0, 0, 2]);
    packet[16..20].copy_from_slice(&[203, 0, 113, 10]);
    packet[20..22].copy_from_slice(&52_000_u16.to_be_bytes());
    packet[20 + 2..20 + 4].copy_from_slice(&443_u16.to_be_bytes());
    packet[20 + 12] = 5 << 4;
    packet[20 + 13] = 0x12;
    let parsed = ParsedPacket::parse(&packet).expect("应解析 IPv4 TCP");
    assert_eq!(parsed.destination_port, Some(443));
    assert_eq!(
        parsed.remote_address(Direction::Upload),
        "203.0.113.10".parse::<IpAddr>().unwrap()
    );
    assert_eq!(parsed.remote_port(Direction::Upload), Some(443));
    assert_eq!(
        parsed.remote_address(Direction::Download),
        "10.0.0.2".parse::<IpAddr>().unwrap()
    );
    assert_eq!(parsed.remote_port(Direction::Download), Some(52_000));
    assert!(parsed.tcp_flags.contains(&TcpFlag::SynAck));
}

#[test]
fn parses_ipv6_udp_remote_metadata_in_both_directions() {
    let mut packet = vec![0_u8; 48];
    packet[0] = 0x60;
    packet[6] = 17;
    packet[8..24].copy_from_slice(&"2001:db8::2".parse::<Ipv6Addr>().unwrap().octets());
    packet[24..40].copy_from_slice(&"2001:db8:1::53".parse::<Ipv6Addr>().unwrap().octets());
    packet[40..42].copy_from_slice(&53_000_u16.to_be_bytes());
    packet[42..44].copy_from_slice(&53_u16.to_be_bytes());
    let parsed = ParsedPacket::parse(&packet).expect("应解析 IPv6 UDP");
    assert_eq!(
        parsed.remote_address(Direction::Upload),
        "2001:db8:1::53".parse::<IpAddr>().unwrap()
    );
    assert_eq!(parsed.remote_port(Direction::Upload), Some(53));
    assert_eq!(
        parsed.remote_address(Direction::Download),
        "2001:db8::2".parse::<IpAddr>().unwrap()
    );
    assert_eq!(parsed.remote_port(Direction::Download), Some(53_000));
}

#[tokio::test]
async fn scheduler_preserves_sequence_without_reorder() {
    let (sender, receiver) = mpsc::channel(8);
    let (observed_sender, mut observed_receiver) = mpsc::channel(8);
    let due = TokioInstant::now();
    for sequence in 0..3_u64 {
        sender
            .send(ScheduledPacket {
                due,
                sequence,
                copies: 1,
                route: PacketRoute::Forward,
                packet: vec![u8::try_from(sequence).expect("测试序号可放入 u8")],
            })
            .await
            .expect("调度输入存在");
    }
    drop(sender);

    run_packet_scheduler(receiver, CancellationToken::new(), move |route, packet| {
        let observed_sender = observed_sender.clone();
        async move {
            assert_eq!(route, PacketRoute::Forward);
            observed_sender
                .send(packet)
                .await
                .map_err(|error| io::Error::other(error.to_string()))
        }
    })
    .await
    .expect("调度成功");

    let mut observed = Vec::new();
    while let Some(packet) = observed_receiver.recv().await {
        observed.push(packet[0]);
    }
    assert_eq!(observed, vec![0, 1, 2]);
}

#[test]
fn fragments_ipv4_packet_to_requested_path_mtu() {
    let packet = test_ipv4_packet(1_500);
    let fragments = fragment_ipv4_packet(&packet, 576).expect("IPv4 应完成分片");

    assert_eq!(fragments.len(), 3);
    assert!(fragments.iter().all(|fragment| fragment.len() <= 576));
    assert!(
        fragments
            .iter()
            .all(|fragment| checksum(&fragment[..20]) == 0)
    );
    assert_eq!(
        u16::from_be_bytes([fragments[0][6], fragments[0][7]]),
        0x2000
    );
    assert_eq!(
        u16::from_be_bytes([fragments[1][6], fragments[1][7]]),
        0x2045
    );
    assert_eq!(
        u16::from_be_bytes([fragments[2][6], fragments[2][7]]),
        0x008a
    );
    let reassembled: Vec<u8> = fragments
        .iter()
        .flat_map(|fragment| fragment[20..].iter().copied())
        .collect();
    assert_eq!(reassembled, packet[20..]);
}

#[test]
fn builds_valid_icmpv4_fragmentation_needed() {
    let packet = test_ipv4_packet(1_500);
    let signal =
        build_icmpv4_fragmentation_needed(&packet, 576).expect("IPv4 应生成 Fragmentation Needed");

    assert_eq!(&signal[12..16], &packet[16..20]);
    assert_eq!(&signal[16..20], &packet[12..16]);
    assert_eq!(&signal[20..22], &[3, 4]);
    assert_eq!(u16::from_be_bytes([signal[26], signal[27]]), 576);
    assert_eq!(checksum(&signal[..20]), 0);
    assert_eq!(checksum(&signal[20..]), 0);
    assert_eq!(&signal[28..], &packet[..28]);
}

#[test]
fn builds_valid_icmpv6_packet_too_big() {
    let packet = test_ipv6_packet(1_500);
    let signal = build_icmpv6_packet_too_big(&packet, 1_280).expect("IPv6 应生成 Packet Too Big");

    assert_eq!(&signal[8..24], &packet[24..40]);
    assert_eq!(&signal[24..40], &packet[8..24]);
    assert_eq!(&signal[40..42], &[2, 0]);
    assert_eq!(
        u32::from_be_bytes(signal[44..48].try_into().unwrap()),
        1_280
    );
    let payload_len = u32::try_from(signal.len() - 40).unwrap();
    let mut pseudo = Vec::new();
    pseudo.extend_from_slice(&signal[8..40]);
    pseudo.extend_from_slice(&payload_len.to_be_bytes());
    pseudo.extend_from_slice(&[0, 0, 0, 58]);
    pseudo.extend_from_slice(&signal[40..]);
    assert_eq!(checksum(&pseudo), 0);
}

#[test]
fn pmtu_construction_failure_is_an_explicit_data_plane_error() {
    let packet = test_ipv6_packet(1_500);
    let mut decision = crate::PacketDecision::pass(&[]);
    decision.path_mtu_action = PathMtuAction::FragmentIpv4(576);

    let error = prepare_path_mtu_packets(&packet, &decision, 1)
        .expect_err("IPv4 分片动作不得静默转发 IPv6 原包");

    assert!(
        error
            .to_string()
            .contains("无法为当前 IP 包执行路径 MTU 动作")
    );
    assert!(error.to_string().contains("恢复系统直连"));
}

#[test]
fn data_plane_drop_cancels_and_reaps_cooperative_thread() {
    let cancellation = CancellationToken::new();
    let thread_cancellation = cancellation.clone();
    let (finished_tx, finished_rx) = sync_mpsc::sync_channel(1);
    let (observed_tx, observed_rx) = sync_mpsc::sync_channel(1);
    let runtime_thread = thread::spawn(move || {
        let _finished = ThreadFinishedNotifier(Some(finished_tx));
        while !thread_cancellation.is_cancelled() {
            thread::sleep(Duration::from_millis(1));
        }
        let _ = observed_tx.send(());
    });
    let handle = DataPlaneHandle {
        runtime_epoch: 0,
        cancellation,
        thread: Some(runtime_thread),
        thread_finished: finished_rx,
        tun_release: None,
    };

    drop(handle);

    observed_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("Drop 应取消并回收协作线程");
}

#[test]
fn data_plane_shutdown_releases_tun_before_its_wait_bound() {
    let (tun, mut peer) = std::os::unix::net::UnixStream::pair().expect("创建测试 TUN 对");
    peer.set_read_timeout(Some(Duration::from_secs(1)))
        .expect("设置读取超时");
    let (managed_tun, tun_release) = ManagedTunFile::new(tun.into()).expect("接管测试 TUN");
    let cancellation = CancellationToken::new();
    let (finished_tx, finished_rx) = sync_mpsc::sync_channel(1);
    let (release_tx, release_rx) = sync_mpsc::sync_channel(1);
    let (observed_tx, observed_rx) = sync_mpsc::sync_channel(1);
    let runtime_thread = thread::spawn(move || {
        let _finished = ThreadFinishedNotifier(Some(finished_tx));
        let _tun = managed_tun;
        let _ = release_rx.recv();
        let _ = observed_tx.send(());
    });
    let mut handle = DataPlaneHandle {
        runtime_epoch: 0,
        cancellation,
        thread: Some(runtime_thread),
        thread_finished: finished_rx,
        tun_release: Some(tun_release),
    };
    let started = Instant::now();

    handle.shutdown_with_timeout(Duration::from_millis(20));

    assert!(started.elapsed() < Duration::from_millis(250));
    let mut byte = [0_u8; 1];
    assert_eq!(
        peer.read(&mut byte).expect("停止后对端应观察到 TUN 关闭"),
        0,
        "即使运行线程不退出，Rust 持有的 TUN 引用也必须先释放"
    );
    release_tx.send(()).expect("释放已分离线程");
    observed_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("分离线程仍应自行结束并释放资源");
}

fn test_ipv4_packet(total_len: usize) -> Vec<u8> {
    let mut packet = vec![0x5a; total_len];
    packet[0] = 0x45;
    packet[1] = 0;
    packet[2..4].copy_from_slice(&u16::try_from(total_len).unwrap().to_be_bytes());
    packet[4..6].copy_from_slice(&0x1234_u16.to_be_bytes());
    packet[6..8].copy_from_slice(&0x4000_u16.to_be_bytes());
    packet[8] = 64;
    packet[9] = 17;
    packet[10..12].fill(0);
    packet[12..16].copy_from_slice(&[10, 0, 0, 2]);
    packet[16..20].copy_from_slice(&[203, 0, 113, 10]);
    let value = checksum(&packet[..20]);
    packet[10..12].copy_from_slice(&value.to_be_bytes());
    packet
}

fn test_ipv6_packet(total_len: usize) -> Vec<u8> {
    let mut packet = vec![0x6b; total_len];
    packet[0] = 0x60;
    packet[4..6].copy_from_slice(&u16::try_from(total_len - 40).unwrap().to_be_bytes());
    packet[6] = 17;
    packet[7] = 64;
    packet[8..24].copy_from_slice(&"2001:db8::2".parse::<Ipv6Addr>().unwrap().octets());
    packet[24..40].copy_from_slice(&"2001:db8::10".parse::<Ipv6Addr>().unwrap().octets());
    packet
}
