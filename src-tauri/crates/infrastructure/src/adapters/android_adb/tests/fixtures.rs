use super::*;

impl AndroidAdbAdapter {
    /// 为单元测试注入可记录、可编排的 ADB 执行器，避免启动真实 adb 进程。
    pub(super) fn with_runner(data_dir: &Path, runner: Arc<dyn AdbCommandRunner>) -> Self {
        Self {
            adb_path: Some(PathBuf::from("adb")),
            companion_apk: Some(data_dir.join("android-companion.apk")),
            selected_serial: RwLock::new(None),
            network_operation: Mutex::new(()),
            active_reverse: Mutex::new(None),
            active_runtime: Mutex::new(None),
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
            weak_network: WeakNetworkProfile::default(),
        },
        proxy_routes: vec![AndroidProxyRouteActivation {
            listener_id: listener_id.to_string(),
            original_destination: destination.into(),
            original_ports: vec![443],
            desktop_listener_bind_address: "0.0.0.0".into(),
            desktop_listener_port,
            allowed_client_cidrs: Vec::new(),
        }],
    }
}

pub(super) async fn seed_active_runtime(
    adapter: &AndroidAdbAdapter,
    serial: &str,
    ports: Vec<u16>,
) -> (ActiveReverseOwnership, ActiveRuntimeFacts) {
    let reverse = ActiveReverseOwnership {
        serial: serial.into(),
        profile_id: "profile-old".into(),
        ports,
    };
    let runtime = ActiveRuntimeFacts {
        serial: serial.into(),
        profile_id: "profile-old".into(),
        profile_fingerprint: "old-profile".into(),
        route_fingerprint: "old-routes".into(),
        route_count: 1,
        uses_adb_reverse: true,
        listener_ports: BTreeMap::new(),
    };
    *adapter.active_reverse.lock().await = Some(reverse.clone());
    *adapter.active_runtime.lock().await = Some(runtime.clone());
    (reverse, runtime)
}

pub(super) fn activation_status(
    state: AndroidNetworkState,
    verified: bool,
    profile_fingerprint: Option<&str>,
    route_fingerprint: Option<&str>,
) -> AndroidNetworkStatusViewModel {
    AndroidNetworkStatusViewModel {
        serial: "2740072778".into(),
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
        serial: "2740072778".into(),
        profile_id: "profile-new".into(),
        profile_fingerprint: "profile-fingerprint".into(),
        route_fingerprint: "route-fingerprint".into(),
        route_count: 2,
        uses_adb_reverse: true,
        listener_ports: BTreeMap::new(),
    }
}
