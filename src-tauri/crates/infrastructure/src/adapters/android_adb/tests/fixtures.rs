use super::*;

impl AndroidAdbAdapter {
    /// 为单元测试注入可记录、可编排的 ADB 执行器，避免启动真实 adb 进程。
    pub(super) fn with_runner(data_dir: &Path, runner: Arc<dyn AdbCommandRunner>) -> Self {
        let runtime_store = Arc::new(crate::SqliteStore::in_memory().expect("runtime store"));
        let sqlite_executor = crate::SqliteExecutor::new(Arc::clone(&runtime_store));
        Self {
            adb_path: Some(PathBuf::from("adb")),
            companion_apk: Some(data_dir.join("android-companion.apk")),
            selected_serial: RwLock::new(None),
            device_operations: DeviceOperationGateRegistry::default(),
            owner_states: Arc::new(Mutex::new(BTreeMap::new())),
            environment_apply_resource_gates: Arc::new(
                crate::adapters::EnvironmentApplyResourceGateRegistry::default(),
            ),
            runtime_store,
            sqlite_executor,
            runner,
            lan_address: Arc::new(NoLanAddressProvider),
        }
    }

    pub(super) fn with_store_and_runner(
        data_dir: &Path,
        runtime_store: Arc<crate::SqliteStore>,
        runner: Arc<dyn AdbCommandRunner>,
    ) -> Self {
        let sqlite_executor = crate::SqliteExecutor::new(Arc::clone(&runtime_store));
        Self {
            adb_path: Some(PathBuf::from("adb")),
            companion_apk: Some(data_dir.join("android-companion.apk")),
            selected_serial: RwLock::new(None),
            device_operations: DeviceOperationGateRegistry::default(),
            owner_states: Arc::new(Mutex::new(BTreeMap::new())),
            environment_apply_resource_gates: Arc::new(
                crate::adapters::EnvironmentApplyResourceGateRegistry::default(),
            ),
            runtime_store,
            sqlite_executor,
            runner,
            lan_address: Arc::new(NoLanAddressProvider),
        }
    }
}

#[derive(Debug)]
struct NoLanAddressProvider;

impl DeviceLanAddressProvider for NoLanAddressProvider {
    fn local_ipv4_for(&self, _: std::net::Ipv4Addr) -> Option<std::net::Ipv4Addr> {
        None
    }
}

#[derive(Debug)]
pub(super) struct FakeRunner;

#[async_trait]
impl AdbCommandRunner for FakeRunner {
    async fn run(&self, _: &Path, _: &[String]) -> std::io::Result<AdbOutput> {
        Ok(AdbOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

#[derive(Debug, Default)]
pub(super) struct RecordingRunner {
    pub(super) calls: std::sync::Mutex<Vec<Vec<String>>>,
}

#[async_trait]
impl AdbCommandRunner for RecordingRunner {
    async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(AdbOutput {
            success: true,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

#[derive(Debug)]
pub(super) struct SequenceRunner {
    pub(super) calls: std::sync::Mutex<Vec<Vec<String>>>,
    pub(super) outputs: std::sync::Mutex<std::collections::VecDeque<AdbOutput>>,
}

#[async_trait]
impl AdbCommandRunner for SequenceRunner {
    async fn run(&self, _: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        Ok(self
            .outputs
            .lock()
            .unwrap()
            .pop_front()
            .expect("测试必须为每次 adb 调用提供结果"))
    }
}

pub(super) fn test_activation(
    profile_id: &str,
    destination: &str,
    listener_id: ListenerId,
    desktop_listener_port: u16,
) -> AndroidNetworkActivation {
    AndroidNetworkActivation {
        profile: AndroidNetworkProfile {
            id: profile_id.into(),
            name: profile_id.into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            proxy_routes: vec![intercept_proxy_domain::AndroidProxyRoute {
                destination: destination.into(),
                ports: vec![443],
                listener_id,
            }],
            confirmed_shared_uids: std::collections::BTreeSet::default(),
            auto_resume_after_reboot: false,
            stop_vpn_on_control_loss: true,
            weak_network: WeakNetworkProfile::default(),
        },
        proxy_routes: vec![AndroidProxyRouteActivation {
            listener_id: listener_id.to_string(),
            listener_name: "Test listener".into(),
            original_destination: destination.into(),
            original_ports: vec![443],
            desktop_listener_bind_address: "0.0.0.0".into(),
            desktop_listener_port,
        }],
    }
}

pub(super) async fn seed_active_runtime(
    adapter: &AndroidAdbAdapter,
    serial: &str,
    ports: Vec<u16>,
) -> (ActiveReverseOwnership, ActiveRuntimeFacts) {
    let epoch = uuid::Uuid::new_v4();
    let reverse = ActiveReverseOwnership {
        epoch,
        serial: serial.into(),
        profile_id: "profile-old".into(),
        ports,
    };
    let runtime = ActiveRuntimeFacts {
        epoch,
        serial: serial.into(),
        profile_id: "profile-old".into(),
        profile_fingerprint: "old-profile".into(),
        route_fingerprint: "old-routes".into(),
        route_count: 1,
        stop_vpn_on_control_loss: true,
        uses_adb_reverse: true,
        listener_ports: BTreeMap::new(),
        endpoints: Vec::new(),
    };
    {
        let mut states = adapter.owner_states.lock().await;
        let state = states.entry(runtime.serial.clone()).or_default();
        state.active_reverse = Some(reverse.clone());
        state.active_runtime = Some(runtime.clone());
    }
    adapter
        .save_owner(runtime.owner(
            AndroidRuntimeOwnerMode::AdbReverse,
            AndroidRuntimeOwnerSource::Start,
            AndroidRuntimeOwnerState::Active,
            AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        ))
        .await
        .unwrap();
    (reverse, runtime)
}

pub(super) fn runtime_owner(
    serial: &str,
    state: AndroidRuntimeOwnerState,
) -> AndroidRuntimeOwnerViewModel {
    AndroidRuntimeOwnerViewModel {
        serial: serial.into(),
        epoch: uuid::Uuid::new_v4(),
        mode: AndroidRuntimeOwnerMode::AdbReverse,
        profile_id: "profile-test".into(),
        state,
        source: AndroidRuntimeOwnerSource::Start,
        transition_reason: AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        updated_at: chrono::Utc::now(),
    }
}

pub(super) fn activation_status(
    state: AndroidNetworkState,
    verified: bool,
    profile_fingerprint: Option<&str>,
    route_fingerprint: Option<&str>,
) -> AndroidNetworkStatusViewModel {
    AndroidNetworkStatusViewModel {
        serial: "2740072778".into(),
        runtime_epoch: None,
        state,
        state_text: String::new(),
        ui_tone: intercept_proxy_application::UiTone::Warning,
        verified,
        transport: AndroidControlTransport::LocalAbstractSocket,
        active_profile_id: Some("profile-new".into()),
        active_profile_fingerprint: profile_fingerprint.map(str::to_owned),
        active_route_fingerprint: route_fingerprint.map(str::to_owned),
        active_route_count: 2,
        companion_process_running: Some(true),
        message: "test status".into(),
        unsupported_fields: Vec::new(),
        stats: None,
    }
}

pub(super) fn activation_runtime() -> ActiveRuntimeFacts {
    ActiveRuntimeFacts {
        epoch: uuid::Uuid::new_v4(),
        serial: "2740072778".into(),
        profile_id: "profile-new".into(),
        profile_fingerprint: "profile-fingerprint".into(),
        route_fingerprint: "route-fingerprint".into(),
        route_count: 2,
        stop_vpn_on_control_loss: true,
        uses_adb_reverse: true,
        listener_ports: BTreeMap::new(),
        endpoints: Vec::new(),
    }
}

pub(super) fn prepared_runtime(owner: AndroidRuntimeOwnerViewModel) -> PreparedUsbProxyRuntime {
    let runtime = ActiveRuntimeFacts {
        epoch: owner.epoch,
        serial: owner.serial.clone(),
        profile_id: owner.profile_id.clone(),
        profile_fingerprint: "profile-fingerprint".into(),
        route_fingerprint: "route-fingerprint".into(),
        route_count: 1,
        stop_vpn_on_control_loss: true,
        listener_ports: BTreeMap::new(),
        uses_adb_reverse: true,
        endpoints: Vec::new(),
    };
    PreparedUsbProxyRuntime {
        payload: serde_json::json!({}),
        reverse: None,
        runtime,
        owner,
        previous_owner: None,
        previous_resume_state: None,
        previous_reverse: None,
        previous_runtime: None,
        previous_endpoints: Vec::new(),
    }
}
