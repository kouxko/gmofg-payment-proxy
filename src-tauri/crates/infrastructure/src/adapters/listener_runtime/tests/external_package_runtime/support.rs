use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppResult, BreakpointCoordinator, EventHub, ExternalPackageApplicationPort,
    InMemorySessionStore, ListenerRuntimePort, ListenerRuntimeState, ProtocolPackageUsageCount,
    ProtocolPackageUsageQueryPort, ProtocolPackageUsageViewModel,
};
use intercept_proxy_domain::{
    ListenerDataPlane, ListenerId, ProtocolPackageRef, ProxyListener, ProxyWorkspace,
    RuleDefinition, ScriptedSocketProcessing, SocketDownstreamSecurity, SocketEndpoint,
    SocketLocalResponderTopology, SocketPayloadProcessing, SocketRelaySecurity,
    SocketRelaySettings, SocketRelayTopology, SocketTopology, WorkspaceId,
};
use intercept_proxy_package_contract::PackageManifest;
use intercept_proxy_product_api::InterceptProxyProfile;
use parking_lot::Mutex;
use serde_json::json;
use tokio::{net::TcpListener, time::timeout};

use super::super::{test_listener_runtime, *};
use crate::{
    ExternalPackageServer, SqliteStore, WorkspaceRecord,
    adapters::{
        CaptureRepositoryAdapter, ExternalPackageRegistryAdapter, ExternalPackageServerConfig,
        PackageTransportConfig, RuleRepositoryAdapter, WorkspaceBodyCodecResolver,
        bundle::{ListenerRuntimePipelineAssembly, configure_listener_runtime_pipeline},
        common::decode_workspace_record,
    },
};

pub(super) const TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[path = "support/peer.rs"]
mod peer;
use peer::TestExternalPeer;

#[derive(Debug)]
struct ListenerUsage {
    current: Mutex<Option<ListenerUsageRecord>>,
}

#[derive(Debug)]
struct ListenerUsageRecord {
    workspace_id: intercept_proxy_domain::WorkspaceId,
    listener_id: ListenerId,
    package: ProtocolPackageRef,
}

#[async_trait]
impl ProtocolPackageUsageQueryPort for ListenerUsage {
    async fn usages(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        Ok(self
            .current
            .lock()
            .as_ref()
            .and_then(|current| {
                (package == &current.package).then(|| ProtocolPackageUsageViewModel {
                    workspace_id: current.workspace_id,
                    workspace_name: "External E2E".into(),
                    listener_id: current.listener_id,
                    listener_name: "External listener".into(),
                    listener_enabled: true,
                    runtime_state: ListenerRuntimeState::Running,
                })
            })
            .into_iter()
            .collect())
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        Ok(Vec::new())
    }
}

pub(super) struct ExternalRuntimeHarness {
    pub(super) runtime: Arc<ListenerRuntimeAdapter>,
    pub(super) registry: Arc<ExternalPackageRegistryAdapter>,
    pub(super) package: ProtocolPackageRef,
    store: Arc<SqliteStore>,
    server: ExternalPackageServer,
    peer: Option<TestExternalPeer>,
    usage: Arc<ListenerUsage>,
}

impl ExternalRuntimeHarness {
    pub(super) async fn start() -> Self {
        let server_address = reserve_address().await;
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let runtime = Arc::new(test_listener_runtime(Arc::clone(&store)));
        let product = InterceptProxyProfile;
        let sessions = Arc::new(InMemorySessionStore::default());
        configure_listener_runtime_pipeline(
            &runtime,
            ListenerRuntimePipelineAssembly {
                product: &product,
                rules: Arc::new(RuleRepositoryAdapter::new(Arc::clone(&store))),
                sessions: Arc::clone(&sessions),
                breakpoints: Arc::new(BreakpointCoordinator::default()),
                events: Arc::new(EventHub::new(16)),
                capture: Arc::new(CaptureRepositoryAdapter::new(sessions)),
                workspace_body_codecs: Arc::new(WorkspaceBodyCodecResolver::new()),
            },
        );
        let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::clone(&store)));
        runtime.set_external_package_provider(registry.clone());
        let registration = registration();
        let package = registration.package().identity().clone();
        let usage = Arc::new(ListenerUsage {
            current: Mutex::new(None),
        });
        let server = ExternalPackageServer::start(
            ExternalPackageServerConfig {
                bind_address: server_address,
                connection: PackageTransportConfig::new(
                    Duration::from_secs(30),
                    Duration::from_secs(10),
                    Duration::from_secs(30),
                    8 * 1024 * 1024,
                    8 * 1024 * 1024,
                    1024 * 1024,
                    128 * 1024,
                ),
            },
            Arc::clone(&registry),
            usage.clone(),
            runtime.clone(),
        )
        .await;
        let peer = TestExternalPeer::spawn(server_address, registration);
        wait_until_package_online(&registry, &package).await;
        registry.set_enabled(&package, true).await.unwrap();
        Self {
            runtime,
            registry,
            package,
            store,
            server,
            peer: Some(peer),
            usage,
        }
    }

    pub(super) async fn start_listener(
        &mut self,
        mut workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) {
        *self.usage.current.lock() = Some(ListenerUsageRecord {
            workspace_id: workspace.id,
            listener_id: listener.id,
            package: self.package.clone(),
        });
        workspace.listeners = vec![listener.clone()];
        self.store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value: encode_workspace_record(&workspace).unwrap(),
                updated_at: Utc::now(),
            })
            .unwrap();
        self.runtime.start(workspace, listener).await.unwrap();
    }

    pub(super) async fn stop_listener(&self, listener_id: ListenerId) {
        self.runtime.stop(listener_id).await.unwrap();
    }

    pub(super) fn workspace(&self, workspace_id: WorkspaceId) -> ProxyWorkspace {
        decode_workspace_record(
            self.store
                .load_workspace(workspace_id.as_uuid())
                .unwrap()
                .expect("persisted external runtime workspace"),
        )
        .unwrap()
    }

    pub(super) async fn disconnect_peer(&mut self) {
        self.peer.take().unwrap().close().await;
    }

    pub(super) async fn wait_until_offline(&self) {
        timeout(TEST_TIMEOUT, async {
            loop {
                if self
                    .registry
                    .get(&self.package)
                    .await
                    .unwrap()
                    .is_some_and(|version| version.source.external_online() == Some(false))
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("external package must become offline");
    }

    pub(super) async fn wait_until_listener_stopped(&self, listener_id: ListenerId) {
        timeout(TEST_TIMEOUT, async {
            loop {
                if self
                    .runtime
                    .statuses()
                    .await
                    .unwrap()
                    .iter()
                    .all(|status| status.listener_id != listener_id)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("external disconnect must stop exact listener");
    }

    pub(super) async fn shutdown(mut self) {
        if let Some(peer) = self.peer.take() {
            peer.close().await;
        }
        self.server.shutdown().await;
    }

    pub(super) fn peer(&self) -> &TestExternalPeer {
        self.peer.as_ref().unwrap()
    }
}

pub(super) async fn reserve_address() -> SocketAddr {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    address
}

pub(super) fn external_relay_listener(
    bind: SocketAddr,
    upstream: SocketAddr,
    package: &ProtocolPackageRef,
) -> ProxyListener {
    external_listener(
        bind,
        SocketTopology::Relay(SocketRelayTopology {
            upstream: SocketEndpoint {
                host: upstream.ip().to_string(),
                port: upstream.port(),
            },
            security: SocketRelaySecurity::Transparent,
        }),
        package,
    )
}

pub(super) fn external_local_listener(
    bind: SocketAddr,
    package: &ProtocolPackageRef,
) -> ProxyListener {
    external_listener(
        bind,
        SocketTopology::LocalResponder(SocketLocalResponderTopology {
            downstream_security: SocketDownstreamSecurity::Tcp,
        }),
        package,
    )
}

fn external_listener(
    bind: SocketAddr,
    topology: SocketTopology,
    package: &ProtocolPackageRef,
) -> ProxyListener {
    ProxyListener {
        name: "External package E2E".into(),
        bind_address: bind.ip().to_string(),
        port: bind.port(),
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
            topology,
            maximum_connections: 8,
            runtime_limits: intercept_proxy_domain::SocketRuntimeLimits::default(),
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: package.clone(),
            }),
        }),
        ..ProxyListener::default()
    }
}

pub(super) fn external_workspace(
    listener: ProxyListener,
    rules: Vec<RuleDefinition>,
) -> ProxyWorkspace {
    let high_water = rules
        .iter()
        .map(RuleDefinition::created_order)
        .max()
        .unwrap_or(0);
    let mut workspace = ProxyWorkspace {
        name: "External E2E".into(),
        listeners: vec![listener],
        rule_created_order_high_water: high_water,
        ..ProxyWorkspace::default()
    };
    workspace.rule_definitions = rules;
    workspace
}

fn registration() -> PackageManifest {
    serde_json::from_value(json!({
        "api": 1,
        "kind": "socket",
        "package": {
            "id": "external-listener-e2e",
            "name": "External listener E2E",
            "version": "1.0.0",
            "description": "test"
        },
        "document": {
            "upstream": {
                "schema": {"type": "object", "title": "Up",
                    "properties": {"payload": {"type": "array", "title": "Payload", "items": {"type": "number"}}}}
            },
            "downstream": {
                "schema": {"type": "object", "title": "Down",
                    "properties": {"payload": {"type": "array", "title": "Down", "items": {"type": "number"}}}}
            }
        }
    }))
    .unwrap()
}

async fn wait_until_package_online(
    registry: &ExternalPackageRegistryAdapter,
    package: &ProtocolPackageRef,
) {
    timeout(TEST_TIMEOUT, async {
        loop {
            if registry
                .get(package)
                .await
                .unwrap()
                .is_some_and(|version| version.source.external_online() == Some(true))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("external package registration deadline");
}
