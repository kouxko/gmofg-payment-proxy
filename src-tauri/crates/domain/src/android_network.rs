//! Android 定向弱网的通用配置模型。
//!
//! 这些类型同时供桌面应用层、Android 数据面和生成的 TypeScript 绑定使用，避免三端
//! 分别维护字段名称、默认值和枚举。这里只描述配置，不依赖 Tauri、ADB、JNI 或网络
//! 运行时，因此未来的 CLI/TUI 也能直接复用。

use serde::{Deserialize, Serialize};
use specta::Type;

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
