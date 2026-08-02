use std::collections::BTreeSet;

pub use intercept_proxy_domain::{
    BitCorruptionProfile, BlackoutWindow, BurstLossProfile, NthTcpFlagDrop, PacketDirection,
    PathMtuProfile, PmtuMode, TcpFlag, WeakNetworkProfile,
};
use serde::{Deserialize, Serialize};

/// Android 上已安装应用的可信快照。
///
/// 该信息必须由 Companion 在每次启动前重新从 `PackageManager` 获取，不能相信桌面端
/// 缓存，因为 APK 可能已经升级、卸载或被不同签名的包替换。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstalledApplication {
    pub package_name: String,
    pub signing_sha256: String,
    pub uid: u32,
}

/// Profile 中选中的目标应用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TargetApplication {
    pub package_name: String,
    pub signing_sha256: String,
    pub uid: u32,
}

/// 一个需要实施弱网的远端 IP/CIDR 与端口集合。
///
/// `ports` 为空代表该地址范围的所有端口。Profile 中可保存多个目标；整个列表为空
/// 时代表目标应用访问的全部原始地址，避免把通用应用错误限制到单一服务器。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DestinationTarget {
    pub cidr: String,
    pub ports: Vec<u16>,
}

/// Android Companion 的完整可持久化 Profile。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkProfile {
    pub id: String,
    pub name: String,
    pub target_applications: Vec<TargetApplication>,
    #[serde(default)]
    pub destination_targets: Vec<DestinationTarget>,
    /// shared UID 组必须整体选中，并由用户显式确认后把 UID 写入此集合。
    pub confirmed_shared_uids: BTreeSet<u32>,
    pub auto_resume_after_reboot: bool,
    pub weak_network: WeakNetworkProfile,
}

/// 引擎内部沿用短名称，序列化契约由领域层的 `PacketDirection` 定义。
pub type Direction = PacketDirection;

/// IP 版本。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpVersion {
    V4,
    V6,
}

/// 传输层协议。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportProtocol {
    Tcp,
    Udp,
    Other,
}

/// 引擎做决定所需的最小包描述。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PacketContext<'a> {
    pub elapsed_millis: u64,
    pub direction: Direction,
    pub ip_version: IpVersion,
    pub transport: TransportProtocol,
    pub destination_port: Option<u16>,
    /// 当前方向的远端地址。上行取 IP destination，下行取 IP source。
    pub remote_address: Option<std::net::IpAddr>,
    /// 当前方向的远端端口。上行取 destination port，下行取 source port。
    pub remote_port: Option<u16>,
    pub tcp_flags: BTreeSet<TcpFlag>,
    pub packet_len: usize,
    pub payload: &'a [u8],
}

/// 包被丢弃的可观测原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    Blackout,
    DnsBlackhole,
    RandomLoss,
    BurstLoss,
    NthTcpFlag,
    PmtuBlackhole,
}

/// 路径 MTU 模拟要求数据面执行的动作。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "mtu")]
pub enum PathMtuAction {
    None,
    ClampMss(u16),
    FragmentIpv4(u16),
    Icmpv4FragmentationNeeded(u16),
    Icmpv6PacketTooBig(u16),
}

/// 单个包的确定性处理结果。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PacketDecision {
    pub drop_reason: Option<DropReason>,
    pub delay_millis: u64,
    pub reorder_hold_millis: u64,
    pub copies: u8,
    pub path_mtu_action: PathMtuAction,
    pub payload: Vec<u8>,
}

impl PacketDecision {
    /// fail-open 的默认结果：不丢包、不修改、立即放行。
    #[must_use]
    pub fn pass(payload: &[u8]) -> Self {
        Self {
            drop_reason: None,
            delay_millis: 0,
            reorder_hold_millis: 0,
            copies: 1,
            path_mtu_action: PathMtuAction::None,
            payload: payload.to_vec(),
        }
    }
}

/// 自启动以来的聚合统计。
///
/// Android 的 shared UID 无法可靠拆分到单个包名，因此这里只按方向聚合，绝不伪造
/// 单应用统计。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EngineStats {
    pub packets_seen: u64,
    pub packets_forwarded: u64,
    pub packets_dropped: u64,
    pub bytes_seen: u64,
    pub bytes_forwarded: u64,
    pub duplicated_packets: u64,
    pub reordered_packets: u64,
    pub corrupted_packets: u64,
}
