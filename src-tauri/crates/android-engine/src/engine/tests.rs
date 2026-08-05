use std::collections::BTreeSet;

use crate::{
    BitCorruptionProfile, BlackoutWindow, BurstLossProfile, DestinationTarget,
    InstalledApplication, NetworkProfile, NthTcpFlagDrop, PathMtuProfile, TargetApplication,
};

use super::*;

fn validated(mut weak: WeakNetworkProfile) -> ValidatedProfile {
    if weak.seed == 0 {
        weak.seed = 7;
    }
    let installed = vec![InstalledApplication {
        package_name: "com.example.target".to_owned(),
        uid: 10001,
    }];
    NetworkProfile {
        id: "test".to_owned(),
        name: "测试".to_owned(),
        target_applications: vec![TargetApplication {
            package_name: "com.example.target".to_owned(),
            uid: 10001,
        }],
        destination_targets: Vec::new(),
        proxy_routes: Vec::new(),
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        weak_network: weak,
    }
    .validate_for_start(&installed)
    .expect("测试 Profile 应合法")
}

fn packet(elapsed_millis: u64, payload: &[u8]) -> PacketContext<'_> {
    PacketContext {
        elapsed_millis,
        direction: Direction::Upload,
        ip_version: IpVersion::V4,
        transport: TransportProtocol::Tcp,
        destination_port: Some(443),
        remote_address: Some("203.0.113.10".parse().expect("测试 IP")),
        remote_port: Some(443),
        tcp_flags: BTreeSet::new(),
        packet_len: payload.len() + 40,
        payload,
    }
}

#[test]
fn same_seed_produces_same_sequence() {
    let weak = WeakNetworkProfile {
        seed: 123,
        fixed_delay_millis: 50,
        uniform_jitter_millis: 20,
        random_loss_basis_points: 1_500,
        duplicate_basis_points: 2_000,
        reorder_basis_points: 2_000,
        maximum_reorder_hold_millis: 100,
        ..WeakNetworkProfile::default()
    };
    let profile = validated(weak);
    let mut first = ImpairmentEngine::new(&profile);
    let mut second = ImpairmentEngine::new(&profile);
    let payload = [1, 2, 3, 4];
    for index in 0..100 {
        assert_eq!(
            first.evaluate(&packet(index, &payload)),
            second.evaluate(&packet(index, &payload))
        );
    }
}

#[test]
fn multiple_destination_targets_apply_faults_only_to_matching_remote_addresses() {
    let installed = vec![InstalledApplication {
        package_name: "com.example.target".to_owned(),
        uid: 10001,
    }];
    let profile = NetworkProfile {
        id: "multiple-addresses".to_owned(),
        name: "多个地址".to_owned(),
        target_applications: vec![TargetApplication {
            package_name: "com.example.target".to_owned(),
            uid: 10001,
        }],
        destination_targets: vec![
            DestinationTarget {
                cidr: "203.0.113.0/24".to_owned(),
                ports: vec![443],
            },
            DestinationTarget {
                cidr: "2001:db8::/32".to_owned(),
                ports: Vec::new(),
            },
        ],
        proxy_routes: Vec::new(),
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        weak_network: WeakNetworkProfile {
            random_loss_basis_points: 10_000,
            ..WeakNetworkProfile::default()
        },
    }
    .validate_for_start(&installed)
    .expect("多个目标地址应通过校验");
    let mut engine = ImpairmentEngine::new(&profile);

    let matching = packet(0, &[1]);
    assert_eq!(
        engine.evaluate(&matching).drop_reason,
        Some(DropReason::RandomLoss)
    );

    let mut unmatched_address = packet(1, &[2]);
    unmatched_address.remote_address = Some("198.51.100.20".parse().unwrap());
    assert_eq!(
        engine.evaluate(&unmatched_address),
        PacketDecision::pass(&[2])
    );

    let mut unmatched_port = packet(2, &[3]);
    unmatched_port.remote_port = Some(80);
    assert_eq!(engine.evaluate(&unmatched_port), PacketDecision::pass(&[3]));

    let mut matching_ipv6 = packet(3, &[4]);
    matching_ipv6.remote_address = Some("2001:db8::99".parse().unwrap());
    matching_ipv6.remote_port = Some(8443);
    assert_eq!(
        engine.evaluate(&matching_ipv6).drop_reason,
        Some(DropReason::RandomLoss)
    );
}

#[test]
fn blackout_and_dns_are_terminal() {
    let profile = validated(WeakNetworkProfile {
        blackout_windows: vec![BlackoutWindow {
            start_after_millis: 100,
            duration_millis: 50,
        }],
        dns_blackhole: true,
        ..WeakNetworkProfile::default()
    });
    let mut engine = ImpairmentEngine::new(&profile);
    assert_eq!(
        engine.evaluate(&packet(120, &[1])).drop_reason,
        Some(DropReason::Blackout)
    );
    let mut dns = packet(200, &[1]);
    dns.transport = TransportProtocol::Udp;
    dns.destination_port = Some(853);
    assert_eq!(
        engine.evaluate(&dns).drop_reason,
        Some(DropReason::DnsBlackhole)
    );
}

#[test]
fn random_and_burst_loss_support_certain_drop_profiles() {
    let random_profile = validated(WeakNetworkProfile {
        random_loss_basis_points: 10_000,
        ..WeakNetworkProfile::default()
    });
    let mut random_engine = ImpairmentEngine::new(&random_profile);
    assert_eq!(
        random_engine.evaluate(&packet(0, &[1])).drop_reason,
        Some(DropReason::RandomLoss)
    );

    let burst_profile = validated(WeakNetworkProfile {
        burst_loss: Some(BurstLossProfile {
            enter_bad_state_basis_points: 10_000,
            leave_bad_state_basis_points: 0,
            good_state_loss_basis_points: 0,
            bad_state_loss_basis_points: 10_000,
        }),
        ..WeakNetworkProfile::default()
    });
    let mut burst_engine = ImpairmentEngine::new(&burst_profile);
    assert_eq!(
        burst_engine.evaluate(&packet(0, &[1])).drop_reason,
        Some(DropReason::BurstLoss)
    );
}

#[test]
fn duplicate_reorder_jitter_and_stats_are_observable() {
    let profile = validated(WeakNetworkProfile {
        fixed_delay_millis: 50,
        uniform_jitter_millis: 10,
        duplicate_basis_points: 10_000,
        reorder_basis_points: 10_000,
        maximum_reorder_hold_millis: 100,
        ..WeakNetworkProfile::default()
    });
    let mut engine = ImpairmentEngine::new(&profile);
    let decision = engine.evaluate(&packet(0, &[1, 2, 3]));
    assert!((40..=60).contains(&decision.delay_millis));
    assert_eq!(decision.copies, 2);
    assert!(decision.reorder_hold_millis <= 100);
    assert_eq!(
        engine.stats(),
        EngineStats {
            packets_seen: 1,
            packets_forwarded: 1,
            packets_dropped: 0,
            bytes_seen: 43,
            bytes_forwarded: 86,
            duplicated_packets: 1,
            reordered_packets: 1,
            corrupted_packets: 0,
        }
    );
}

#[test]
fn nth_syn_is_dropped_exactly_once() {
    let profile = validated(WeakNetworkProfile {
        nth_tcp_flag_drops: vec![NthTcpFlagDrop {
            direction: Direction::Upload,
            flag: TcpFlag::Syn,
            nth: 2,
        }],
        ..WeakNetworkProfile::default()
    });
    let mut engine = ImpairmentEngine::new(&profile);
    let mut syn = packet(0, &[]);
    syn.tcp_flags = BTreeSet::from([TcpFlag::Syn]);
    assert_eq!(engine.evaluate(&syn).drop_reason, None);
    assert_eq!(
        engine.evaluate(&syn).drop_reason,
        Some(DropReason::NthTcpFlag)
    );
    assert_eq!(engine.evaluate(&syn).drop_reason, None);
}

#[test]
fn every_supported_tcp_flag_and_direction_can_be_dropped_exactly_at_n() {
    let flags = [
        TcpFlag::Syn,
        TcpFlag::SynAck,
        TcpFlag::Ack,
        TcpFlag::Fin,
        TcpFlag::Rst,
    ];
    let directions = [Direction::Upload, Direction::Download];

    for direction in directions {
        for flag in flags {
            let profile = validated(WeakNetworkProfile {
                nth_tcp_flag_drops: vec![NthTcpFlagDrop {
                    direction,
                    flag,
                    nth: 2,
                }],
                ..WeakNetworkProfile::default()
            });
            let mut engine = ImpairmentEngine::new(&profile);
            let mut candidate = packet(0, &[]);
            candidate.direction = direction;
            candidate.tcp_flags = BTreeSet::from([flag]);

            assert_eq!(
                engine.evaluate(&candidate).drop_reason,
                None,
                "{direction:?} {flag:?} 的第 1 个包不应被丢弃"
            );
            assert_eq!(
                engine.evaluate(&candidate).drop_reason,
                Some(DropReason::NthTcpFlag),
                "{direction:?} {flag:?} 的第 2 个包应被丢弃"
            );
            assert_eq!(
                engine.evaluate(&candidate).drop_reason,
                None,
                "{direction:?} {flag:?} 的第 3 个包不应被丢弃"
            );
        }
    }
}

#[test]
fn rate_limit_schedules_following_packet() {
    let profile = validated(WeakNetworkProfile {
        upload_bytes_per_second: Some(1_000),
        ..WeakNetworkProfile::default()
    });
    let mut engine = ImpairmentEngine::new(&profile);
    let first = packet(0, &[0; 960]);
    let second = packet(0, &[0; 960]);
    assert_eq!(engine.evaluate(&first).delay_millis, 0);
    assert_eq!(engine.evaluate(&second).delay_millis, 1_000);
}

#[test]
fn pmtu_and_mss_actions_are_explicit() {
    let profile = validated(WeakNetworkProfile {
        path_mtu: PathMtuProfile {
            mtu: Some(1_280),
            mss_clamp: Some(1_200),
            mode: PmtuMode::SignalTooBig,
        },
        ..WeakNetworkProfile::default()
    });
    let mut engine = ImpairmentEngine::new(&profile);
    let mut syn = packet(0, &[]);
    syn.tcp_flags = BTreeSet::from([TcpFlag::Syn]);
    assert_eq!(
        engine.evaluate(&syn).path_mtu_action,
        PathMtuAction::ClampMss(1_200)
    );
    let mut ipv6 = packet(1, &[0; 1_300]);
    ipv6.ip_version = IpVersion::V6;
    assert_eq!(
        engine.evaluate(&ipv6).path_mtu_action,
        PathMtuAction::Icmpv6PacketTooBig(1_280)
    );
}

#[test]
fn ipv4_fragment_signal_and_blackhole_are_distinct() {
    let oversized = packet(0, &[0; 1_300]);
    let mut fragment = ImpairmentEngine::new(&validated(WeakNetworkProfile {
        path_mtu: PathMtuProfile {
            mtu: Some(1_280),
            mss_clamp: None,
            mode: PmtuMode::FragmentOrPacketTooBig,
        },
        ..WeakNetworkProfile::default()
    }));
    assert_eq!(
        fragment.evaluate(&oversized).path_mtu_action,
        PathMtuAction::FragmentIpv4(1_280)
    );

    let mut signal = ImpairmentEngine::new(&validated(WeakNetworkProfile {
        path_mtu: PathMtuProfile {
            mtu: Some(1_280),
            mss_clamp: None,
            mode: PmtuMode::SignalTooBig,
        },
        ..WeakNetworkProfile::default()
    }));
    assert_eq!(
        signal.evaluate(&oversized).path_mtu_action,
        PathMtuAction::Icmpv4FragmentationNeeded(1_280)
    );

    let mut blackhole = ImpairmentEngine::new(&validated(WeakNetworkProfile {
        path_mtu: PathMtuProfile {
            mtu: Some(1_280),
            mss_clamp: None,
            mode: PmtuMode::Blackhole,
        },
        ..WeakNetworkProfile::default()
    }));
    assert_eq!(
        blackhole.evaluate(&oversized).drop_reason,
        Some(DropReason::PmtuBlackhole)
    );
}

#[test]
fn corruption_changes_payload_but_not_length() {
    let profile = validated(WeakNetworkProfile {
        corruption: BitCorruptionProfile {
            probability_basis_points: 10_000,
            bits_per_packet: 3,
        },
        ..WeakNetworkProfile::default()
    });
    let mut engine = ImpairmentEngine::new(&profile);
    let original = [0_u8; 8];
    let decision = engine.evaluate(&packet(0, &original));
    assert_ne!(decision.payload, original);
    assert_eq!(decision.payload.len(), original.len());
}

#[test]
fn fail_open_wrapper_returns_original_payload() {
    let profile = validated(WeakNetworkProfile::default());
    let engine = FailOpenEngine::new(&profile);
    let context = packet(0, &[1, 2, 3]);
    let (decision, error) = engine.evaluate(&context);
    assert!(error.is_none());
    assert_eq!(decision.payload, context.payload);
    assert_eq!(decision.copies, 1);
}
