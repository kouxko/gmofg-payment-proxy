use std::{
    collections::BTreeMap,
    fmt::Debug,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::PathBuf,
    sync::{Arc, RwLock},
};

use chrono::Utc;
use intercept_proxy_application::{
    AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState,
    AndroidRuntimeOwnerViewModel, AppError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use super::{
    AndroidAdbAdapter,
    command::{SystemAdbCommandRunner, discover_adb, discover_companion_apk},
};

impl AndroidAdbAdapter {
    pub fn new(
        companion_apk: Option<PathBuf>,
        runtime_store: Arc<crate::SqliteStore>,
    ) -> Result<Self, crate::InfrastructureError> {
        // 优先使用桌面外壳解析的安装资源；无界面测试和其他 Host 再按约定位置回退发现。
        let companion_apk = companion_apk
            .filter(|path| path.is_file())
            .or_else(discover_companion_apk);
        let persisted = runtime_store.load_android_runtime_owner()?.map(|mut record| {
            record.owner.source = AndroidRuntimeOwnerSource::Recovery;
            record.owner.transition_reason =
                intercept_proxy_application::AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage;
            record.owner.updated_at = Utc::now();
            record
        });
        if let Some(record) = persisted.as_ref() {
            runtime_store.save_android_runtime_owner(record)?;
        }
        Ok(Self {
            adb_path: discover_adb(),
            companion_apk,
            selected_serial: RwLock::new(None),
            network_operation: Mutex::new(()),
            active_reverse: Mutex::new(persisted.as_ref().and_then(|record| {
                (!record.reverse_ports.is_empty()).then(|| ActiveReverseOwnership {
                    epoch: record.owner.epoch,
                    serial: record.owner.serial.clone(),
                    profile_id: record.owner.profile_id.clone(),
                    ports: record.reverse_ports.clone(),
                })
            })),
            active_runtime: Mutex::new(None),
            runtime_resume_state: Mutex::new(
                persisted.as_ref().and_then(|record| record.resume_state),
            ),
            runtime_owner: Mutex::new(persisted.map(|record| record.owner)),
            runtime_store,
            runner: Arc::new(SystemAdbCommandRunner),
            lan_address: Arc::new(SystemDeviceLanAddressProvider),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ActiveReverseOwnership {
    pub(super) epoch: uuid::Uuid,
    pub(super) serial: String,
    pub(super) profile_id: String,
    pub(super) ports: Vec<u16>,
}

/// 桌面端为当前 Android start/apply 解析出的运行事实。
///
/// 不能从可持久化 Profile 重新推导该值，因为实际端点包含本次 ADB reverse 端口与
/// DNS 解析结果。桌面进程重启后该事实自然丢失，状态核对会 fail-closed，要求重新
/// apply，而不是假定设备仍连接旧端点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ActiveRuntimeFacts {
    pub(super) epoch: uuid::Uuid,
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
    pub(super) owner: AndroidRuntimeOwnerViewModel,
    pub(super) previous_owner: Option<AndroidRuntimeOwnerViewModel>,
    pub(super) previous_resume_state: Option<AndroidRuntimeOwnerState>,
    pub(super) previous_reverse: Option<ActiveReverseOwnership>,
    pub(super) previous_runtime: Option<ActiveRuntimeFacts>,
}

impl ActiveRuntimeFacts {
    pub(super) fn owner(
        &self,
        mode: AndroidRuntimeOwnerMode,
        source: AndroidRuntimeOwnerSource,
        state: AndroidRuntimeOwnerState,
        reason: intercept_proxy_application::AndroidRuntimeOwnerTransitionReason,
    ) -> AndroidRuntimeOwnerViewModel {
        AndroidRuntimeOwnerViewModel {
            serial: self.serial.clone(),
            epoch: self.epoch,
            mode,
            profile_id: self.profile_id.clone(),
            state,
            source,
            transition_reason: reason,
            updated_at: Utc::now(),
        }
    }
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
