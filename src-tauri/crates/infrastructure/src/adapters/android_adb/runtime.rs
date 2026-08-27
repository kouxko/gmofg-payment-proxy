use std::{
    collections::BTreeMap,
    fmt::Debug,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex, RwLock, Weak},
};

use chrono::Utc;
use intercept_proxy_application::{
    AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource,
    AndroidRuntimeOwnerState, AndroidRuntimeOwnerViewModel, AppError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use super::{
    AndroidAdbAdapter,
    command::{SystemAdbCommandRunner, discover_adb, discover_companion_apk},
};

#[derive(Debug, Default, Clone)]
pub(super) struct AndroidOwnerState {
    pub(super) active_reverse: Option<ActiveReverseOwnership>,
    pub(super) active_runtime: Option<ActiveRuntimeFacts>,
    pub(super) runtime_endpoints: Vec<AndroidRuntimeEndpointViewModel>,
    pub(super) runtime_owner: Option<AndroidRuntimeOwnerViewModel>,
    pub(super) runtime_resume_state: Option<AndroidRuntimeOwnerState>,
}

#[derive(Debug, Default)]
pub(super) struct DeviceOperationGateRegistry {
    gates: StdMutex<BTreeMap<String, Weak<Mutex<()>>>>,
}

impl DeviceOperationGateRegistry {
    pub(super) fn gate(&self, serial: &str) -> Arc<Mutex<()>> {
        let mut gates = self.gates.lock().expect("android device gate registry");
        gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = gates.get(serial).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(Mutex::new(()));
        gates.insert(serial.to_owned(), Arc::downgrade(&gate));
        gate
    }
}

impl AndroidAdbAdapter {
    pub async fn new(
        companion_apk: Option<PathBuf>,
        persistence: impl crate::IntoSqlitePersistence,
    ) -> Result<Self, crate::InfrastructureError> {
        let (sqlite_executor, runtime_store) = persistence.into_sqlite_persistence();
        #[cfg(not(test))]
        let _ = &runtime_store;
        // 优先使用桌面外壳解析的安装资源；无界面测试和其他 Host 再按约定位置回退发现。
        let companion_apk = companion_apk
            .filter(|path| path.is_file())
            .or_else(discover_companion_apk);
        let persisted = sqlite_executor
            .execute(|store| {
                let persisted = store.load_android_runtime_owners()?.into_iter().map(|mut record| {
                    record.owner.source = AndroidRuntimeOwnerSource::Recovery;
                    record.owner.transition_reason = intercept_proxy_application::AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage;
                    record.owner.updated_at = Utc::now();
                    record
                }).collect::<Vec<_>>();
                for record in &persisted {
                    store.replace_android_runtime_owner_if_epoch(
                        &record.owner.serial,
                        record.owner.epoch,
                        record,
                    )?;
                }
                Ok::<_, crate::InfrastructureError>(persisted)
            })
            .await?;
        Ok(Self {
            environment_apply_resource_gates: Arc::new(
                super::super::EnvironmentApplyResourceGateRegistry::default(),
            ),
            adb_path: discover_adb(),
            companion_apk,
            selected_serial: RwLock::new(None),
            device_operations: DeviceOperationGateRegistry::default(),
            owner_states: Arc::new(Mutex::new(
                persisted
                    .into_iter()
                    .map(|record| {
                        let serial = record.owner.serial.clone();
                        let state = AndroidOwnerState {
                            active_reverse: (!record.reverse_ports.is_empty()).then(|| {
                                ActiveReverseOwnership {
                                    epoch: record.owner.epoch,
                                    serial: record.owner.serial.clone(),
                                    profile_id: record.owner.profile_id.clone(),
                                    ports: record.reverse_ports.clone(),
                                }
                            }),
                            active_runtime: None,
                            runtime_endpoints: record.runtime_endpoints,
                            runtime_resume_state: record.resume_state,
                            runtime_owner: Some(record.owner),
                        };
                        (serial, state)
                    })
                    .collect(),
            )),
            #[cfg(test)]
            runtime_store,
            sqlite_executor,
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
    pub(super) endpoints: Vec<AndroidRuntimeEndpointViewModel>,
}

#[derive(Debug, Clone)]
pub(super) struct PreparedUsbProxyRuntime {
    pub(super) payload: Value,
    pub(super) reverse: Option<ActiveReverseOwnership>,
    pub(super) runtime: ActiveRuntimeFacts,
    pub(super) owner: AndroidRuntimeOwnerViewModel,
    pub(super) previous_owner: Option<AndroidRuntimeOwnerViewModel>,
    pub(super) previous_resume_state: Option<AndroidRuntimeOwnerState>,
    pub(super) previous_reverse: Option<ActiveReverseOwnership>,
    pub(super) previous_runtime: Option<ActiveRuntimeFacts>,
    pub(super) previous_endpoints: Vec<AndroidRuntimeEndpointViewModel>,
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
