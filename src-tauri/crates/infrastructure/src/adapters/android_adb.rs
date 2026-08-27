use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use tokio::sync::Mutex;

mod command;
mod control_port;
mod device_reconciliation;
mod endpoint_reconciliation;
mod environment_apply;
mod fingerprint;
mod owner;
mod protocol;
mod reverse;
mod runtime;
mod status;

use command::{AdbCommandRunner, parse_devices, parse_package_version, parse_packages};
use endpoint_reconciliation::is_owner_unreachable;
use fingerprint::sha256_json;
use protocol::is_socket_unavailable;
use reverse::{combine_stop_failures, reverse_mapping_present};
use runtime::{
    ActiveReverseOwnership, ActiveRuntimeFacts, AndroidOwnerState, DeviceLanAddressProvider,
    DeviceOperationGateRegistry, PreparedUsbProxyRuntime, ReverseCleanupOutcome,
};
use status::{
    companion_install_view_model, consent_opened_status, control_unavailable_status,
    no_runtime_owner_status, normalize_packages, owner_disconnected_status,
};

#[cfg(test)]
use async_trait::async_trait;
#[cfg(test)]
use command::{AdbOutput, bundled_companion_apk_candidates};
#[cfg(test)]
use fingerprint::canonical_json;
#[cfg(test)]
use intercept_proxy_application::{
    ANDROID_COMPANION_PACKAGE, AndroidControlPort, AndroidControlTransport,
    AndroidNetworkActivation, AndroidNetworkState, AndroidNetworkStatusViewModel,
    AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState,
    AndroidRuntimeOwnerTransitionReason, AndroidRuntimeOwnerViewModel, AppError,
};
#[cfg(test)]
use protocol::{ActivationObservation, classify_activation_status, reconcile_forward_cleanup};
#[cfg(test)]
use reverse::allocated_reverse_ports;
const CONTROL_SOCKET: &str = "intercept_proxy_vpn";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const INSTALL_TIMEOUT: Duration = Duration::from_mins(2);

#[derive(Debug)]
pub struct AndroidAdbAdapter {
    pub(super) environment_apply_resource_gates: Arc<super::EnvironmentApplyResourceGateRegistry>,
    adb_path: Option<PathBuf>,
    companion_apk: Option<PathBuf>,
    selected_serial: RwLock<Option<String>>,
    device_operations: DeviceOperationGateRegistry,
    owner_states: Arc<Mutex<BTreeMap<String, AndroidOwnerState>>>,
    #[cfg(test)]
    runtime_store: Arc<crate::SqliteStore>,
    sqlite_executor: crate::SqliteExecutor,
    runner: Arc<dyn AdbCommandRunner>,
    lan_address: Arc<dyn DeviceLanAddressProvider>,
}

#[cfg(test)]
#[path = "android_adb/tests.rs"]
mod tests;
