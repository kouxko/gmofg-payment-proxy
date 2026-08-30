use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use intercept_proxy_application::{
    AppResult, ListenerRuntimePort, ListenerRuntimeState, ListenerStatusViewModel,
    ListenerUpstreamConnectionTestViewModel, ListenerUpstreamTlsTestViewModel,
    ProtocolPackageUsageCount, ProtocolPackageUsageQueryPort, ProtocolPackageUsageViewModel,
    UiTone,
};
use intercept_proxy_domain::{
    ListenerId, ProtocolPackageRef, ProxyListener, ProxyWorkspace, WorkspaceId,
};
use intercept_proxy_package_contract::{PackageManifest, PackageRegisterNotification};
use parking_lot::Mutex;
use tokio::{io::DuplexStream, sync::Barrier};
use tokio_tungstenite::{
    WebSocketStream, client_async,
    tungstenite::{Message, protocol::Role},
};

use super::*;
use crate::{
    SqliteStore,
    adapters::{
        ExternalPackageConnectionId, accept_packages_websocket,
        external_package_registration_fingerprint,
    },
};

type Peer = WebSocketStream<DuplexStream>;

#[tokio::test]
async fn packages_websocket_uses_rpc_ceiling_but_rejects_rpc_plus_one() {
    let registration_limit = 256;
    let rpc_limit = 1024;
    let config = PackageTransportConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(10),
        Duration::from_secs(30),
        4096,
        rpc_limit,
        registration_limit,
        512,
    );
    assert_eq!(config.websocket_message_bytes(), rpc_limit);
    let (server_io, client_io) = tokio::io::duplex(4096);
    let server = tokio::spawn(async move {
        let mut websocket = accept_packages_websocket(server_io, config.websocket_message_bytes())
            .await
            .expect("/packages handshake");
        let accepted = websocket
            .next()
            .await
            .expect("response message")
            .expect("within rpc ceiling");
        assert_eq!(accepted.into_text().unwrap().len(), rpc_limit);
        websocket
            .next()
            .await
            .expect("oversized result")
            .expect_err("rpc+1 must fail")
    });
    let (mut client, _) = client_async("ws://localhost/packages", client_io)
        .await
        .expect("client handshake");
    client
        .send(Message::Text("x".repeat(rpc_limit).into()))
        .await
        .unwrap();
    client
        .send(Message::Text("x".repeat(rpc_limit + 1).into()))
        .await
        .unwrap();
    let error = server.await.unwrap();
    assert!(
        matches!(error, tokio_tungstenite::tungstenite::Error::Capacity(_)),
        "{error:?}"
    );
}

#[path = "tests/fault_isolation.rs"]
mod fault_isolation;

#[derive(Debug)]
struct BlockingUsage {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
    usages: Vec<ProtocolPackageUsageViewModel>,
}

#[async_trait]
impl ProtocolPackageUsageQueryPort for BlockingUsage {
    async fn usages(
        &self,
        _package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        self.entered.wait().await;
        self.release.wait().await;
        Ok(self.usages.clone())
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        Ok(Vec::new())
    }
}

#[derive(Debug)]
struct TrackingRuntime {
    stopped: Mutex<Vec<ListenerId>>,
    run_token: Mutex<uuid::Uuid>,
    blocking_call: Option<usize>,
    calls: AtomicUsize,
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

impl TrackingRuntime {
    fn immediate() -> Self {
        Self {
            stopped: Mutex::new(Vec::new()),
            run_token: Mutex::new(uuid::Uuid::from_u128(1)),
            blocking_call: None,
            calls: AtomicUsize::new(0),
            entered: Arc::new(Barrier::new(1)),
            release: Arc::new(Barrier::new(1)),
        }
    }

    fn blocking_first_stop() -> Self {
        Self {
            stopped: Mutex::new(Vec::new()),
            run_token: Mutex::new(uuid::Uuid::from_u128(1)),
            blocking_call: Some(0),
            calls: AtomicUsize::new(0),
            entered: Arc::new(Barrier::new(2)),
            release: Arc::new(Barrier::new(2)),
        }
    }

    fn restart(&self) {
        *self.run_token.lock() = uuid::Uuid::from_u128(2);
    }
}

#[async_trait]
impl ListenerRuntimePort for TrackingRuntime {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        Ok(Vec::new())
    }

    async fn start(
        &self,
        _workspace: ProxyWorkspace,
        _listener: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel> {
        unreachable!("the race test models restart by publishing a new package generation")
    }

    async fn stop(&self, listener_id: ListenerId) -> AppResult<ListenerStatusViewModel> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.stopped.lock().push(listener_id);
        if self.blocking_call == Some(call) {
            self.entered.wait().await;
            self.release.wait().await;
        }
        Ok(stopped_status(listener_id))
    }

    async fn replace_rule_definitions(
        &self,
        _workspace: ProxyWorkspace,
        _listener_id: ListenerId,
    ) -> AppResult<()> {
        unreachable!("not used by disconnect cleanup")
    }

    async fn test_upstream_connection(
        &self,
        _workspace: ProxyWorkspace,
        _listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
        unreachable!("not used by disconnect cleanup")
    }

    async fn test_upstream_tls(
        &self,
        _workspace: ProxyWorkspace,
        _listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        unreachable!("not used by disconnect cleanup")
    }
}

#[async_trait]
impl ExternalPackageListenerRuntime for TrackingRuntime {
    async fn current_run_token(&self, _listener_id: ListenerId) -> Option<uuid::Uuid> {
        Some(*self.run_token.lock())
    }

    async fn stop_if_run_token(
        &self,
        listener_id: ListenerId,
        expected_run_token: uuid::Uuid,
    ) -> AppResult<Option<ListenerStatusViewModel>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.blocking_call == Some(call) {
            self.entered.wait().await;
            self.release.wait().await;
        }
        if *self.run_token.lock() != expected_run_token {
            return Ok(None);
        }
        self.stopped.lock().push(listener_id);
        Ok(Some(stopped_status(listener_id)))
    }
}

#[tokio::test]
async fn reconnect_while_usage_query_is_blocked_prevents_stale_listener_stop() {
    let (registry, package, disconnected_connection_id, _first_peer) =
        disconnected_registry(1).await;
    let listener_id = ListenerId::new();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let usage = Arc::new(BlockingUsage {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
        usages: vec![running_usage(listener_id)],
    });
    let runtime = Arc::new(TrackingRuntime::immediate());

    let cleanup = spawn_cleanup(
        Arc::clone(&registry),
        package.clone(),
        disconnected_connection_id,
        usage,
        runtime.clone(),
    );
    entered.wait().await;
    let _second_peer = reconnect(&registry, 2).await;
    release.wait().await;
    cleanup.await.expect("cleanup task");

    assert!(runtime.stopped.lock().is_empty());
}

#[tokio::test]
async fn reconnect_during_first_stop_prevents_stale_cleanup_from_stopping_remaining_listener() {
    let (registry, package, disconnected_connection_id, _first_peer) =
        disconnected_registry(10).await;
    let first_listener = ListenerId::new();
    let second_listener = ListenerId::new();
    let usage = Arc::new(FixedUsage(vec![
        running_usage(first_listener),
        running_usage(second_listener),
    ]));
    let runtime = Arc::new(TrackingRuntime::blocking_first_stop());

    let cleanup = spawn_cleanup(
        Arc::clone(&registry),
        package,
        disconnected_connection_id,
        usage,
        runtime.clone(),
    );
    runtime.entered.wait().await;
    let _second_peer = reconnect(&registry, 11).await;
    runtime.release.wait().await;
    cleanup.await.expect("cleanup task");

    assert_eq!(runtime.stopped.lock().as_slice(), &[first_listener]);
}

#[tokio::test]
async fn restarted_listener_is_not_removed_after_offline_check_completed() {
    let (registry, package, disconnected_connection_id, _first_peer) =
        disconnected_registry(20).await;
    let listener_id = ListenerId::new();
    let usage = Arc::new(FixedUsage(vec![running_usage(listener_id)]));
    let runtime = Arc::new(TrackingRuntime::blocking_first_stop());

    let cleanup = spawn_cleanup(
        Arc::clone(&registry),
        package,
        disconnected_connection_id,
        usage,
        runtime.clone(),
    );
    // Reaching the conditional-stop barrier proves the registry offline check has completed and
    // the cleanup task has captured the old runtime token.
    runtime.entered.wait().await;
    let _second_peer = reconnect(&registry, 21).await;
    runtime.restart();
    runtime.release.wait().await;
    cleanup.await.expect("cleanup task");

    assert!(runtime.stopped.lock().is_empty());
}

#[derive(Debug)]
struct FixedUsage(Vec<ProtocolPackageUsageViewModel>);

#[async_trait]
impl ProtocolPackageUsageQueryPort for FixedUsage {
    async fn usages(
        &self,
        _package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        Ok(self.0.clone())
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        Ok(Vec::new())
    }
}

fn spawn_cleanup(
    registry: Arc<ExternalPackageRegistryAdapter>,
    package: ProtocolPackageRef,
    disconnected_connection_id: ExternalPackageConnectionId,
    usage: Arc<dyn ProtocolPackageUsageQueryPort>,
    runtime: Arc<dyn ExternalPackageListenerRuntime>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        stop_exact_package_listeners(
            &package,
            disconnected_connection_id,
            registry.as_ref(),
            usage.as_ref(),
            runtime.as_ref(),
        )
        .await;
    })
}

async fn disconnected_registry(
    generation: u64,
) -> (
    Arc<ExternalPackageRegistryAdapter>,
    ProtocolPackageRef,
    ExternalPackageConnectionId,
    Peer,
) {
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::new(
        SqliteStore::in_memory().expect("in-memory store"),
    )));
    let registration = registration();
    let package = registration.package().identity().clone();
    let (client, peer) = connected_client(&registration, generation).await;
    let accepted = registry
        .accept_registration(
            &registration,
            external_package_registration_fingerprint(&registration).expect("fingerprint"),
            client,
        )
        .await
        .expect("first registration");
    assert!(
        registry
            .mark_disconnected(&package, accepted.connection_id)
            .await
    );
    (registry, package, accepted.connection_id, peer)
}

async fn reconnect(registry: &ExternalPackageRegistryAdapter, generation: u64) -> Peer {
    let registration = registration();
    let (client, peer) = connected_client(&registration, generation).await;
    registry
        .accept_registration(
            &registration,
            external_package_registration_fingerprint(&registration).expect("fingerprint"),
            client,
        )
        .await
        .expect("reconnect registration");
    peer
}

async fn connected_client(
    registration: &PackageManifest,
    generation: u64,
) -> (PackageTransportClient, Peer) {
    let (actor_io, peer_io) = tokio::io::duplex(2 * 1024 * 1024);
    let actor = WebSocketStream::from_raw_socket(actor_io, Role::Server, None).await;
    let mut peer = WebSocketStream::from_raw_socket(peer_io, Role::Client, None).await;
    let config = PackageTransportConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(10),
        Duration::from_secs(30),
        1024 * 1024,
        1024 * 1024,
        1024 * 1024,
        128 * 1024,
    );
    let connecting = tokio::spawn(PackageTransportClient::connect(actor, generation, config));
    peer.send(Message::Text(
        serde_json::to_string(&PackageRegisterNotification::new(registration.clone()))
            .expect("registration notification")
            .into(),
    ))
    .await
    .expect("registration response");
    let (returned, client) = connecting
        .await
        .expect("actor task")
        .expect("registered client");
    assert_eq!(&returned, registration);
    (client, peer)
}

fn running_usage(listener_id: ListenerId) -> ProtocolPackageUsageViewModel {
    ProtocolPackageUsageViewModel {
        workspace_id: WorkspaceId::new(),
        workspace_name: "Race workspace".into(),
        listener_id,
        listener_name: "Race listener".into(),
        listener_enabled: true,
        runtime_state: ListenerRuntimeState::Running,
    }
}

fn stopped_status(listener_id: ListenerId) -> ListenerStatusViewModel {
    ListenerStatusViewModel {
        listener_id,
        runtime_epoch: None,
        state: ListenerRuntimeState::Stopped,
        state_text: "已停止".into(),
        ui_tone: UiTone::Neutral,
        listen_address: String::new(),
        fault_reason: None,
        can_start: true,
        can_stop: false,
        active_connections: 0,
        client_to_server_bytes: 0,
        server_to_client_bytes: 0,
        retained_diagnostic_evictions: 0,
    }
}

fn registration() -> PackageManifest {
    registration_with_id("race-iso8583")
}

fn registration_with_id(package_id: &str) -> PackageManifest {
    serde_json::from_value(serde_json::json!({
        "api": 1,
        "kind": "socket",
        "package": {
            "id": package_id, "name": "Race ISO8583", "version": "1.0.0",
            "description": "reconnect race test"
        },
        "document": {
            "upstream": {
                "schema": {
                    "type": "object", "title": "Upstream",
                    "properties": {"mti": {"type": "string", "title": "MTI"}}
                }
            },
            "downstream": {
                "schema": {
                    "type": "object", "title": "Downstream",
                    "properties": {"response_code": {"type": "string", "title": "RC"}}
                }
            }
        }
    }))
    .expect("valid registration")
}
