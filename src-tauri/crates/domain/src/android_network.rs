//! Android 应用网络接管的通用配置模型。
//!
//! 这些类型同时供桌面应用层、Android 数据面和生成的 TypeScript 绑定使用，避免三端
//! 分别维护字段名称、默认值和枚举。这里只描述配置，不依赖 Tauri、ADB、JNI 或网络
//! 运行时，因此未来的 CLI/TUI 也能直接复用。

use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{DomainError, ErrorCode, ListenerId};

pub const ANDROID_COMPANION_PACKAGE: &str = "com.interceptproxy.vpn";

/// 一段相对于弱网引擎启动时刻的断网窗口。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct BlackoutWindow {
    pub start_after_millis: u64,
    pub duration_millis: u64,
}

/// Gilbert-Elliott 两状态突发丢包模型，概率统一使用 0..=10000 基点。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct BurstLossProfile {
    pub enter_bad_state_basis_points: u16,
    pub leave_bad_state_basis_points: u16,
    pub good_state_loss_basis_points: u16,
    pub bad_state_loss_basis_points: u16,
}

/// 包在 TUN 中的移动方向。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PacketDirection {
    Upload,
    Download,
}

/// 需要精确计数的 TCP 标志位。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TcpFlag {
    Syn,
    SynAck,
    Ack,
    Fin,
    Rst,
}

/// 丢弃某个方向上的第 N 个指定 TCP 标志包。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct NthTcpFlagDrop {
    pub direction: PacketDirection,
    pub flag: TcpFlag,
    pub nth: u64,
}

impl Default for NthTcpFlagDrop {
    fn default() -> Self {
        Self {
            direction: PacketDirection::Upload,
            flag: TcpFlag::Syn,
            nth: 1,
        }
    }
}

/// 超过路径 MTU 时的处理语义。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PmtuMode {
    #[default]
    Pass,
    FragmentOrPacketTooBig,
    SignalTooBig,
    Blackhole,
}

/// 路径 MTU 与 TCP MSS 故障配置。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct PathMtuProfile {
    pub mtu: Option<u16>,
    pub mss_clamp: Option<u16>,
    pub mode: PmtuMode,
}

/// TCP/UDP Payload 位翻转配置。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct BitCorruptionProfile {
    pub probability_basis_points: u16,
    pub bits_per_packet: u8,
}

/// 纯 Rust 弱网配置，也是桌面端、Companion 与数据面的唯一字段契约。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct WeakNetworkProfile {
    pub seed: u64,
    pub fixed_delay_millis: u64,
    pub uniform_jitter_millis: u64,
    pub upload_bytes_per_second: Option<u64>,
    pub download_bytes_per_second: Option<u64>,
    pub random_loss_basis_points: u16,
    pub burst_loss: Option<BurstLossProfile>,
    pub duplicate_basis_points: u16,
    pub reorder_basis_points: u16,
    pub maximum_reorder_hold_millis: u64,
    pub blackout_windows: Vec<BlackoutWindow>,
    pub dns_blackhole: bool,
    pub nth_tcp_flag_drops: Vec<NthTcpFlagDrop>,
    pub path_mtu: PathMtuProfile,
    pub corruption: BitCorruptionProfile,
}

impl Default for WeakNetworkProfile {
    fn default() -> Self {
        Self {
            seed: 1,
            fixed_delay_millis: 0,
            uniform_jitter_millis: 0,
            upload_bytes_per_second: None,
            download_bytes_per_second: None,
            random_loss_basis_points: 0,
            burst_loss: None,
            duplicate_basis_points: 0,
            reorder_basis_points: 0,
            maximum_reorder_hold_millis: 0,
            blackout_windows: Vec::new(),
            dns_blackhole: false,
            nth_tcp_flag_drops: Vec::new(),
            path_mtu: PathMtuProfile::default(),
            corruption: BitCorruptionProfile::default(),
        }
    }
}

/// Android 设备网络方案锁定的目标应用快照。
/// 包名用于建立 `VpnService` allowlist，UID 用于 shared UID 整组校验；不检查 APK 签名。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidTargetApplication {
    pub package_name: String,
    pub uid: u32,
    pub display_name: Option<String>,
}

/// 弱网只作用于指定远端地址/端口时使用的过滤条件。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidDestinationTarget {
    pub cidr: String,
    pub ports: Vec<u16>,
}

/// 将指定原始目标透明转交给 Workspace 中的一条桌面代理监听。
/// 这里只保存用户输入的目标和稳定 Listener 引用。桌面 IP、ADB reverse 端口、SOCKS
/// 地址和设备端 transport 均是启动时解析的运行态，禁止进入 Workspace 文档。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidProxyRoute {
    pub destination: String,
    pub ports: Vec<u16>,
    pub listener_id: ListenerId,
}

/// 可随 Workspace 导入导出的 Android 设备网络方案。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidNetworkProfile {
    pub id: String,
    pub name: String,
    pub target_applications: Vec<AndroidTargetApplication>,
    /// 空列表表示目标应用的全部远端地址都进入弱网引擎。
    pub destination_targets: Vec<AndroidDestinationTarget>,
    /// 空列表表示不将流量透明转交桌面代理，只执行设备端弱网。
    pub proxy_routes: Vec<AndroidProxyRoute>,
    pub confirmed_shared_uids: BTreeSet<u32>,
    pub auto_resume_after_reboot: bool,
    #[serde(default = "default_stop_vpn_on_control_loss")]
    pub stop_vpn_on_control_loss: bool,
    pub weak_network: WeakNetworkProfile,
}

const fn default_stop_vpn_on_control_loss() -> bool {
    true
}

impl AndroidNetworkProfile {
    pub fn validate(&self) -> Result<(), DomainError> {
        let mut fields = BTreeMap::<String, Vec<String>>::new();
        if self.id.is_empty() || self.id.len() > 128 || !is_safe_profile_id(&self.id) {
            fields.insert(
                "id".into(),
                vec![
                    "设备网络方案 ID 只能包含字母、数字、点、下划线和连字符，且不超过 128 字节。"
                        .into(),
                ],
            );
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            fields.insert(
                "name".into(),
                vec!["设备网络方案名称不能为空且不能超过 80 个字符。".into()],
            );
        }
        validate_target_applications(self, &mut fields);
        validate_destination_targets(self, &mut fields);
        validate_proxy_routes(self, &mut fields);
        if serde_json::to_vec(&self.weak_network).map_or(true, |bytes| bytes.len() > 256 * 1024) {
            fields.insert(
                "weak_network".into(),
                vec!["弱网配置必须是对象且不能超过 256 KiB。".into()],
            );
        }
        if fields.is_empty() {
            Ok(())
        } else {
            let mut error = DomainError::new(ErrorCode::ConfigInvalid, "设备网络方案校验失败");
            error.field_errors = Box::new(fields);
            Err(error)
        }
    }

    #[must_use]
    pub fn requires_dangerous_confirmation(&self) -> bool {
        self.weak_network.random_loss_basis_points >= 10_000
            || !self.weak_network.blackout_windows.is_empty()
    }
}

fn validate_target_applications(
    profile: &AndroidNetworkProfile,
    fields: &mut BTreeMap<String, Vec<String>>,
) {
    if profile.target_applications.is_empty() || profile.target_applications.len() > 64 {
        fields.insert(
            "target_applications".into(),
            vec!["必须选择 1 到 64 个目标应用。".into()],
        );
    }
    let mut packages = BTreeSet::new();
    for (index, target) in profile.target_applications.iter().enumerate() {
        let prefix = format!("target_applications.{index}");
        if target.package_name == ANDROID_COMPANION_PACKAGE {
            fields
                .entry(prefix.clone())
                .or_default()
                .push("设备端组件自身不能进入网络接管允许列表。".into());
        }
        if !is_android_package_name(&target.package_name) || !packages.insert(&target.package_name)
        {
            fields
                .entry(format!("{prefix}.package_name"))
                .or_default()
                .push("包名无效或重复。".into());
        }
        if target.uid == 0 {
            fields
                .entry(format!("{prefix}.uid"))
                .or_default()
                .push("UID 必须大于 0。".into());
        }
    }
}

fn validate_destination_targets(
    profile: &AndroidNetworkProfile,
    fields: &mut BTreeMap<String, Vec<String>>,
) {
    if profile.destination_targets.len() > 128 {
        fields.insert(
            "destination_targets".into(),
            vec!["一个设备网络方案最多配置 128 个弱网覆盖地址范围。".into()],
        );
    }
    let mut unique = BTreeSet::new();
    for (index, target) in profile.destination_targets.iter().enumerate() {
        let prefix = format!("destination_targets.{index}");
        let normalized_destination = normalize_android_ip_cidr(&target.cidr);
        if normalized_destination.is_none() {
            fields
                .entry(format!("{prefix}.cidr"))
                .or_default()
                .push("请输入单个 IP 或合法 IPv4/IPv6 CIDR。".into());
        }
        validate_ports(&target.ports, &prefix, fields);
        let mut ports = target.ports.clone();
        ports.sort_unstable();
        if !unique.insert((
            normalized_destination.unwrap_or_else(|| target.cidr.trim().to_owned()),
            ports,
        )) {
            fields
                .entry(prefix)
                .or_default()
                .push("目标地址范围与端口组合不能重复。".into());
        }
    }
}

fn validate_proxy_routes(
    profile: &AndroidNetworkProfile,
    fields: &mut BTreeMap<String, Vec<String>>,
) {
    if profile.proxy_routes.len() > 128 {
        fields.insert(
            "proxy_routes".into(),
            vec!["一个设备网络方案最多配置 128 条透明代理路由。".into()],
        );
    }
    let mut unique = BTreeSet::new();
    for (index, route) in profile.proxy_routes.iter().enumerate() {
        let prefix = format!("proxy_routes.{index}");
        let normalized_destination = normalize_android_network_destination(&route.destination);
        if normalized_destination.is_none() {
            fields
                .entry(format!("{prefix}.destination"))
                .or_default()
                .push("请输入主机名、单个 IP 或合法 IPv4/IPv6 CIDR。".into());
        }
        validate_ports(&route.ports, &prefix, fields);
        let destination =
            normalized_destination.unwrap_or_else(|| route.destination.trim().to_owned());
        for port in &route.ports {
            if !unique.insert((destination.clone(), *port)) {
                fields
                    .entry(prefix.clone())
                    .or_default()
                    .push("同一目标地址与端口只能配置一条透明代理路由。".into());
                break;
            }
        }
    }
}

fn validate_ports(ports: &[u16], prefix: &str, fields: &mut BTreeMap<String, Vec<String>>) {
    let mut unique = BTreeSet::new();
    if ports.iter().any(|port| *port == 0 || !unique.insert(*port)) {
        fields
            .entry(format!("{prefix}.ports"))
            .or_default()
            .push("端口必须位于 1..=65535 且不能重复。".into());
    }
}

fn is_safe_profile_id(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_android_package_name(value: &str) -> bool {
    value.len() <= 255
        && value.contains('.')
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

/// 将 Android 路由中的单个 IP 或 CIDR 转换为唯一、稳定的文本形式。
///
/// IPv6 会压缩为标准写法；CIDR 的主机位会被清零，确保等价网段无法绕过重复校验。
#[must_use]
pub fn normalize_android_ip_cidr(value: &str) -> Option<String> {
    let value = value.trim();
    let (address, prefix) = value
        .split_once('/')
        .map_or((value, None), |(address, prefix)| (address, Some(prefix)));
    let address = address.parse::<IpAddr>().ok()?;
    let maximum = if address.is_ipv4() { 32 } else { 128 };
    let prefix = prefix.map_or(Some(maximum), |prefix| prefix.parse::<u8>().ok())?;
    if prefix > maximum {
        return None;
    }

    match address {
        IpAddr::V4(address) => {
            let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
            Some(format!(
                "{}/{}",
                Ipv4Addr::from(u32::from(address) & mask),
                prefix
            ))
        }
        IpAddr::V6(address) => {
            let mask = u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0);
            Some(format!(
                "{}/{}",
                Ipv6Addr::from(u128::from(address) & mask),
                prefix
            ))
        }
    }
}

/// 将透明代理原始目标规范化为唯一键。
///
/// 主机名会去掉末尾根域点并转为小写；IP/CIDR 使用
/// [`normalize_android_ip_cidr`] 的标准形式。
#[must_use]
pub fn normalize_android_network_destination(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_suffix('.').unwrap_or(value);
    if value.ends_with('.') {
        return None;
    }
    if let Some(normalized) = normalize_android_ip_cidr(value) {
        return Some(normalized);
    }
    let value = value.to_ascii_lowercase();
    (!value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        }))
    .then_some(value)
}
