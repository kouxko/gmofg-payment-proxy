use super::*;

#[derive(Debug)]
struct RunningAndroidControl {
    status: AndroidNetworkStatusViewModel,
    observed_activation: parking_lot::Mutex<Option<AndroidNetworkActivation>>,
    runtime_ready: AtomicBool,
    network_start_calls: AtomicUsize,
    network_apply_calls: AtomicUsize,
    block_start: AtomicBool,
    start_entered: tokio::sync::Notify,
    start_release: tokio::sync::Notify,
}

#[async_trait]
impl AndroidControlPort for RunningAndroidControl {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        unused()
    }
    async fn adb_select(&self, _: String) -> AppResult<AndroidAdbViewModel> {
        unused()
    }
    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        unused()
    }
    async fn package_list(&self) -> AppResult<Vec<AndroidPackageViewModel>> {
        Ok(vec![AndroidPackageViewModel {
            package_name: "example.target".into(),
            uid: 10_001,
            shared_uid: None,
        }])
    }
    async fn package_get(&self, _: String) -> AppResult<AndroidPackageViewModel> {
        unused()
    }
    async fn companion_install(&self, _: bool) -> AppResult<AndroidCompanionInstallViewModel> {
        unused()
    }
    async fn vpn_open_consent(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_start(
        &self,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        self.network_start_calls.fetch_add(1, Ordering::SeqCst);
        self.start_entered.notify_one();
        if self.block_start.load(Ordering::SeqCst) {
            self.start_release.notified().await;
        }
        Ok(self.status.clone())
    }
    async fn network_apply(
        &self,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        self.network_apply_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.status.clone())
    }
    async fn network_runtime_ready(
        &self,
        activation: &AndroidNetworkActivation,
        _: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool> {
        *self.observed_activation.lock() = Some(activation.clone());
        Ok(self.runtime_ready.load(Ordering::SeqCst))
    }
    async fn network_stop(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn emergency_restore(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_status(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        Ok(self.status.clone())
    }
    async fn runtime_owner(&self) -> AppResult<Option<AndroidRuntimeOwnerViewModel>> {
        Ok(None)
    }
    async fn network_runtime_endpoints(
        &self,
        _: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
        Ok(Vec::new())
    }
}

#[derive(Debug)]
struct StaticListenerRuntime {
    statuses: Vec<ListenerStatusViewModel>,
}

#[async_trait]
impl ListenerRuntimePort for StaticListenerRuntime {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        Ok(self.statuses.clone())
    }

    async fn start(
        &self,
        _: ProxyWorkspace,
        _: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel> {
        unused()
    }

    async fn stop(&self, _: ListenerId) -> AppResult<ListenerStatusViewModel> {
        unused()
    }

    async fn replace_protocol_rules(&self, _: ProxyWorkspace, _: ListenerId) -> AppResult<()> {
        Ok(())
    }

    async fn test_upstream_tls(
        &self,
        _: ProxyWorkspace,
        _: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        unused()
    }

    async fn test_upstream_connection(
        &self,
        _: ProxyWorkspace,
        _: ProxyListener,
    ) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
        unused()
    }
}

struct RunningVpnFixture {
    application: Application,
    android: Arc<RunningAndroidControl>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    original_id: WorkspaceId,
    profile_id: String,
    listener_id: ListenerId,
}

fn running_android_control(profile_id: &str, stale_profile: bool) -> Arc<RunningAndroidControl> {
    Arc::new(RunningAndroidControl {
        status: AndroidNetworkStatusViewModel {
            serial: "device-1".into(),
            state: AndroidNetworkState::Running,
            state_text: "运行中".into(),
            ui_tone: UiTone::Positive,
            verified: true,
            transport: AndroidControlTransport::LocalAbstractSocket,
            active_profile_id: Some(if stale_profile {
                "已经不在本地配置中的方案".into()
            } else {
                profile_id.into()
            }),
            active_profile_fingerprint: Some("profile-fingerprint".into()),
            active_route_fingerprint: Some("route-fingerprint".into()),
            active_route_count: 1,
            companion_process_running: Some(true),
            message: "运行中".into(),
            unsupported_fields: Vec::new(),
            stats: None,
        },
        observed_activation: parking_lot::Mutex::new(None),
        runtime_ready: AtomicBool::new(true),
        network_start_calls: AtomicUsize::new(0),
        network_apply_calls: AtomicUsize::new(0),
        block_start: AtomicBool::new(false),
        start_entered: tokio::sync::Notify::new(),
        start_release: tokio::sync::Notify::new(),
    })
}

async fn running_vpn_fixture_with_stale_profile(stale_profile: bool) -> RunningVpnFixture {
    running_vpn_fixture_with_listener_state(stale_profile, None).await
}

async fn running_vpn_fixture_with_listener_state(
    stale_profile: bool,
    listener_state: Option<ListenerRuntimeState>,
) -> RunningVpnFixture {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let original_id = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.selected)
        .expect("selected default workspace")
        .id;
    let mut original = workspaces
        .get(original_id)
        .await
        .expect("default workspace");
    let listener_id = original.listeners[0].id;
    original.listeners[0].enabled = true;
    original.listeners[0].port = 41_273;
    let profile_id = Uuid::new_v4().to_string();
    original
        .android_network_profiles
        .push(AndroidNetworkProfile {
            id: profile_id.clone(),
            name: "运行中的方案".into(),
            target_applications: vec![AndroidTargetApplication {
                package_name: "example.target".into(),
                uid: 10_001,
                display_name: Some("测试应用".into()),
            }],
            destination_targets: Vec::new(),
            proxy_routes: vec![intercept_proxy_domain::AndroidProxyRoute {
                destination: "service.example.test".into(),
                ports: vec![41_273],
                listener_id,
            }],
            confirmed_shared_uids: BTreeSet::default(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        });
    workspaces.save(original.clone()).await.unwrap();
    let other = workspaces.create("其他 Workspace".into()).await.unwrap();
    workspaces.select(other.id).await.unwrap();

    let android = running_android_control(&profile_id, stale_profile);
    let listener_runtime = Arc::new(StaticListenerRuntime {
        statuses: listener_state
            .map(|state| ListenerStatusViewModel {
                listener_id,
                state,
                state_text: "测试状态".into(),
                ui_tone: UiTone::Neutral,
                listen_address: "127.0.0.1:41273".into(),
                fault_reason: None,
                can_start: false,
                can_stop: false,
                active_connections: 0,
                client_to_server_bytes: 0,
                server_to_client_bytes: 0,
                retained_diagnostic_evictions: 0,
            })
            .into_iter()
            .collect(),
    });
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            workspaces: workspaces.clone(),
            listener_runtime,
            protocol_packages: unused_protocol_package_services(),
            events: Arc::new(EventHub::default()),
        },
        android.clone(),
        Arc::new(UnusedProtectedSecretPort),
    );

    RunningVpnFixture {
        application,
        android,
        workspaces,
        original_id,
        profile_id,
        listener_id,
    }
}

async fn running_vpn_fixture() -> RunningVpnFixture {
    running_vpn_fixture_with_stale_profile(false).await
}
