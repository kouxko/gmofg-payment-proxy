use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use intercept_proxy_application::{
    AppResult, ExternalPackageApplicationPort, ListenerRuntimePort, ListenerRuntimeState,
    ProtocolPackageUsageCount, ProtocolPackageUsageQueryPort, ProtocolPackageUsageViewModel,
};
use intercept_proxy_domain::{
    ListenerDataPlane, ListenerId, ProtocolDocumentRuleDefinition, ProtocolPackageRef,
    ProxyListener, ProxyWorkspace, ScriptedSocketProcessing, SocketDownstreamSecurity,
    SocketEndpoint, SocketLocalResponderTopology, SocketPayloadProcessing, SocketRelaySecurity,
    SocketRelaySettings, SocketRelayTopology, SocketTopology,
};
use intercept_proxy_package_contract::{
    CanonicalBase64, DecodeParams, EncodeParams, FrameParams, FrameResult, PackageManifest,
    PackageRegisterNotification,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::timeout,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::super::{test_listener_runtime, *};
use crate::{
    ExternalPackageServer, SqliteStore,
    adapters::{
        ExternalPackageRegistryAdapter, ExternalPackageServerConfig, PackageTransportConfig,
    },
};

pub(super) const TEST_TIMEOUT: Duration = Duration::from_secs(5);

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
    server: ExternalPackageServer,
    peer: Option<TestExternalPeer>,
    usage: Arc<ListenerUsage>,
}

impl ExternalRuntimeHarness {
    pub(super) async fn start() -> Self {
        let server_address = reserve_address().await;
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let runtime = Arc::new(test_listener_runtime(Arc::clone(&store)));
        let registry = Arc::new(ExternalPackageRegistryAdapter::new(store));
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
        self.runtime.start(workspace, listener).await.unwrap();
    }

    pub(super) async fn stop_listener(&self, listener_id: ListenerId) {
        self.runtime.stop(listener_id).await.unwrap();
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

pub(super) struct TestExternalPeer {
    registrations: Arc<AtomicUsize>,
    invalid_boundary_once: Arc<AtomicBool>,
    need_more: tokio::sync::Mutex<mpsc::UnboundedReceiver<()>>,
    close: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl TestExternalPeer {
    fn spawn(address: SocketAddr, registration: PackageManifest) -> Self {
        let registrations = Arc::new(AtomicUsize::new(0));
        let invalid_boundary_once = Arc::new(AtomicBool::new(false));
        let (need_more_tx, need_more_rx) = mpsc::unbounded_channel();
        let (close_tx, mut close_rx) = oneshot::channel();
        let task_registrations = Arc::clone(&registrations);
        let task_invalid_boundary_once = Arc::clone(&invalid_boundary_once);
        let task = tokio::spawn(async move {
            let (mut socket, _) = timeout(
                TEST_TIMEOUT,
                connect_async(format!("ws://{address}/packages")),
            )
            .await
            .expect("external peer connection deadline")
            .expect("external peer WebSocket connection");
            socket
                .send(Message::Text(
                    serde_json::to_string(&PackageRegisterNotification::new(registration))
                        .unwrap()
                        .into(),
                ))
                .await
                .unwrap();
            task_registrations.fetch_add(1, Ordering::AcqRel);
            loop {
                tokio::select! {
                    _ = &mut close_rx => {
                        socket.close(None).await.unwrap();
                        break;
                    }
                    incoming = socket.next() => {
                        let Some(incoming) = incoming else { break };
                        match incoming.unwrap() {
                            Message::Text(text) => respond(
                                &mut socket,
                                &task_invalid_boundary_once,
                                &need_more_tx,
                                &text,
                            ).await,
                            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
                            Message::Close(_) => break,
                            Message::Pong(_) => {}
                            other => panic!("unexpected WebSocket message: {other:?}"),
                        }
                    }
                }
            }
        });
        Self {
            registrations,
            invalid_boundary_once,
            need_more: tokio::sync::Mutex::new(need_more_rx),
            close: Some(close_tx),
            task,
        }
    }

    pub(super) fn registration_count(&self) -> usize {
        self.registrations.load(Ordering::Acquire)
    }

    pub(super) fn return_oversized_frame_boundary_once(&self) {
        self.invalid_boundary_once.store(true, Ordering::Release);
    }

    pub(super) async fn wait_for_need_more(&self) {
        timeout(TEST_TIMEOUT, self.need_more.lock().await.recv())
            .await
            .expect("frame NeedMore observation deadline")
            .expect("frame NeedMore observation");
    }

    async fn close(mut self) {
        if let Some(close) = self.close.take() {
            let _ = close.send(());
        }
        timeout(TEST_TIMEOUT, self.task)
            .await
            .expect("external peer close deadline")
            .expect("external peer task");
    }
}

async fn respond<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    invalid_boundary_once: &AtomicBool,
    need_more: &mpsc::UnboundedSender<()>,
    text: &str,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let request: Value = serde_json::from_str(text).unwrap();
    let method = request["method"].as_str().unwrap();
    let result = match method {
        "hooks.upstream.frame" | "hooks.downstream.frame" => {
            let frame: FrameParams = serde_json::from_value(request["params"].clone()).unwrap();
            let bytes = frame.buffer.bytes();
            let boundary = if invalid_boundary_once.swap(false, Ordering::AcqRel) {
                FrameResult::complete(bytes.len() + 1).unwrap()
            } else if bytes
                .first()
                .is_some_and(|length| bytes.len() >= usize::from(*length))
            {
                FrameResult::complete(usize::from(bytes[0])).unwrap()
            } else {
                let _ = need_more.send(());
                FrameResult::NeedMore {
                    required_bytes: bytes.first().copied().map(usize::from),
                }
            };
            serde_json::to_value(boundary).unwrap()
        }
        "hooks.upstream.decode" | "hooks.downstream.decode" => {
            let decoded: DecodeParams = serde_json::from_value(request["params"].clone()).unwrap();
            let buffer: CanonicalBase64 = decoded.input.try_into().unwrap();
            json!({"payload": buffer.bytes()})
        }
        "hooks.upstream.encode" | "hooks.downstream.encode" => {
            let encoded: EncodeParams = serde_json::from_value(request["params"].clone()).unwrap();
            let document = serde_json::to_value(encoded.document).unwrap();
            let bytes = document["payload"]
                .as_array()
                .and_then(|values| {
                    values
                        .iter()
                        .map(|value| {
                            let value = value.as_f64()?;
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            (value.fract() == 0.0 && (0.0..=255.0).contains(&value))
                                .then_some(value as u8)
                        })
                        .collect::<Option<Vec<_>>>()
                })
                .unwrap_or_else(|| vec![3, b'O', b'K']);
            json!(CanonicalBase64::from_bytes(&bytes).as_str())
        }
        "document.upstream.display" | "document.downstream.display" => json!("<p>external e2e</p>"),
        other => panic!("unexpected external method: {other}"),
    };
    socket
        .send(Message::Text(
            json!({"jsonrpc": "2.0", "id": request["id"], "result": result})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
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
    rules: Vec<ProtocolDocumentRuleDefinition>,
) -> ProxyWorkspace {
    let mut workspace = ProxyWorkspace {
        name: "External E2E".into(),
        listeners: vec![listener],
        ..ProxyWorkspace::default()
    };
    workspace.replace_document_runtime_rules(rules).unwrap();
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
