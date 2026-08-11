use std::{
    collections::BTreeMap,
    fmt::Debug,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::PathBuf,
    sync::{Arc, RwLock},
};

use intercept_proxy_application::AppError;
use serde_json::Value;
use tokio::sync::Mutex;

use super::{
    AndroidAdbAdapter,
    command::{SystemAdbCommandRunner, discover_adb, discover_companion_apk},
};

impl AndroidAdbAdapter {
    #[must_use]
    pub fn new(companion_apk: Option<PathBuf>) -> Self {
        // 优先使用桌面外壳解析的安装资源；无界面测试和其他 Host 再按约定位置回退发现。
        let companion_apk = companion_apk
            .filter(|path| path.is_file())
            .or_else(discover_companion_apk);
        Self {
            adb_path: discover_adb(),
            companion_apk,
            selected_serial: RwLock::new(None),
            network_operation: Mutex::new(()),
            active_reverse: Mutex::new(None),
            active_runtime: Mutex::new(None),
            runner: Arc::new(SystemAdbCommandRunner),
            lan_address: Arc::new(SystemDeviceLanAddressProvider),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActiveReverseOwnership {
    pub(super) serial: String,
    pub(super) profile_id: String,
    pub(super) ports: Vec<u16>,
}

/// 桌面端为当前 Android start/apply 解析出的运行事实。
///
/// 不能从可持久化 Profile 重新推导该值，因为实际端点包含本次 ADB reverse 端口与
/// DNS 解析结果。桌面进程重启后该事实自然丢失，状态核对会 fail-closed，要求重新
/// apply，而不是假定设备仍连接旧端点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActiveRuntimeFacts {
    pub(super) serial: String,
    pub(super) profile_id: String,
    pub(super) profile_fingerprint: String,
    pub(super) route_fingerprint: String,
    pub(super) route_count: usize,
    pub(super) listener_ports: BTreeMap<String, u16>,
    pub(super) uses_adb_reverse: bool,
}

#[derive(Debug)]
pub(super) struct PreparedUsbProxyRuntime {
    pub(super) payload: Value,
    pub(super) reverse: Option<ActiveReverseOwnership>,
    pub(super) runtime: ActiveRuntimeFacts,
}

#[derive(Debug)]
pub(super) struct ReverseCleanupOutcome {
    pub(super) remaining_ports: Vec<u16>,
    pub(super) error: Option<AppError>,
}

pub(super) trait DeviceLanAddressProvider: Debug + Send + Sync {
    fn local_ipv4_for(&self, device_address: Ipv4Addr) -> Option<Ipv4Addr>;
}

#[derive(Debug, Default)]
pub(super) struct SystemDeviceLanAddressProvider;

impl DeviceLanAddressProvider for SystemDeviceLanAddressProvider {
    fn local_ipv4_for(&self, device_address: Ipv4Addr) -> Option<Ipv4Addr> {
        // UDP connect 只让系统选择到设备地址的本地接口，不建立连接也不发送数据。
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        socket.connect((device_address, 9)).ok()?;
        let IpAddr::V4(address) = socket.local_addr().ok()?.ip() else {
            return None;
        };
        (!address.is_unspecified() && !address.is_loopback()).then_some(address)
    }
}
