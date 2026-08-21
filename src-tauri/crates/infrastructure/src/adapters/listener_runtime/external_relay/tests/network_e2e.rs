//! 真实 localhost WebSocket 边界的外部软件包验收测试。
//!
//! 这里刻意不用 duplex 或 `FakeExternalRpc`：第三方测试端通过 `connect_async` 接入
//! Proxy 的真实 `TcpListener`，以覆盖握手、注册 actor、注册表和 Relay processor 的接线。

use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use intercept_proxy_application::{
    AppResult, ExternalPackageApplicationPort, ProtocolPackageUsageCount,
    ProtocolPackageUsageQueryPort, ProtocolPackageUsageViewModel,
};
use intercept_proxy_domain::{
    ExternalDecodeRequest, ExternalDecodeResponse, ExternalDisplayResponse, ExternalEncodeRequest,
    ExternalEncodeResponse, ExternalFrameRequest, ExternalFrameResult, ProtocolPackageRef,
};
use intercept_proxy_runtime::{
    FrameBoundary, ScriptedRelayProcessorFactory, SocketPayloadDirection,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle, time::timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::super::*;
use super::{
    connection, listener_id, network_e2e_runtime::UnusedListenerRuntime, registration, rules,
};
use crate::{
    ExternalPackageConnectionConfig, ExternalPackageRegistryAdapter, ExternalPackageServer,
    ExternalPackageServerConfig, SqliteStore,
    adapters::listener_runtime::ExternalSocketPackageProvider,
};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
struct EmptyUsage;

#[async_trait]
impl ProtocolPackageUsageQueryPort for EmptyUsage {
    async fn usages(
        &self,
        _package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        Ok(Vec::new())
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        Ok(Vec::new())
    }
}

struct ExternalPeer {
    registration_count: Arc<AtomicUsize>,
    methods: Arc<Mutex<Vec<String>>>,
    close: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl ExternalPeer {
    async fn close(mut self) {
        if let Some(close) = self.close.take() {
            let _ = close.send(());
        }
        timeout(TEST_TIMEOUT, self.task)
            .await
            .expect("external peer must close within the test deadline")
            .expect("external peer task must not panic");
    }
}

struct NetworkHarness {
    server: ExternalPackageServer,
    registry: Arc<ExternalPackageRegistryAdapter>,
    peer: ExternalPeer,
    package: ProtocolPackageRef,
}

impl NetworkHarness {
    async fn start() -> Self {
        let address = reserve_loopback_address().await;
        let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::new(
            SqliteStore::in_memory().expect("in-memory registry"),
        )));
        let server = ExternalPackageServer::start(
            ExternalPackageServerConfig {
                bind_address: address,
                connection: ExternalPackageConnectionConfig::default(),
            },
            Arc::clone(&registry),
            Arc::new(EmptyUsage),
            Arc::new(UnusedListenerRuntime),
        )
        .await;
        let registered = registration();
        let package = registered.package().identity().clone();
        let peer = spawn_external_peer(address, registered);
        wait_until_online(&registry, &package).await;
        Self {
            server,
            registry,
            peer,
            package,
        }
    }

    async fn shutdown(self) {
        self.peer.close().await;
        self.server.shutdown().await;
    }
}

#[tokio::test]
async fn real_websocket_registers_once_and_processes_two_relay_frames() {
    let harness = NetworkHarness::start().await;
    harness
        .registry
        .set_enabled(&harness.package, true)
        .await
        .expect("enable external package");
    let binding = ExternalSocketPackageProvider::resolve(&*harness.registry, &harness.package)
        .expect("resolve external package")
        .expect("external package binding");
    let registered = binding.registration().clone();
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        binding,
        rules(&registered),
        intercept_proxy_domain::SocketTopology::default(),
    );
    let factory = ExternalRelayProcessorFactoryAdapter::new(
        &snapshot,
        SocketCaptureContext {
            workspace_id: intercept_proxy_domain::WorkspaceId::new(),
            listener_id: listener_id(),
            publisher: None,
        },
    );
    let mut processor =
        factory.create_direction(connection(), SocketPayloadDirection::AppToUpstream);

    assert_eq!(
        processor
            .inspect(bytes::Bytes::from_static(b"one-two"))
            .await
            .expect("frame boundary"),
        FrameBoundary::Complete { bytes: 3 }
    );
    assert_eq!(
        processor
            .process(bytes::Bytes::from_static(b"one"))
            .await
            .expect("first relay frame"),
        bytes::Bytes::from_static(b"encoded")
    );
    processor.output_committed();
    assert_eq!(
        processor
            .process(bytes::Bytes::from_static(b"two"))
            .await
            .expect("second relay frame"),
        bytes::Bytes::from_static(b"encoded")
    );
    processor.output_committed();
    wait_for_method_count(&harness.peer.methods, 7).await;

    assert_eq!(harness.peer.registration_count.load(Ordering::Acquire), 1);
    let method_counts = {
        let methods = harness.peer.methods.lock();
        (
            method_count(&methods, "hooks.upstream.split_frame"),
            method_count(&methods, "hooks.upstream.decrypt_and_decode"),
            method_count(&methods, "hooks.upstream.encode_and_encrypt"),
            method_count(&methods, "document.upstream.render_message"),
        )
    };
    assert_eq!(method_counts, (1, 2, 2, 2));
    harness.shutdown().await;
}

#[tokio::test]
async fn real_websocket_disconnect_marks_enabled_package_offline() {
    let harness = NetworkHarness::start().await;
    harness
        .registry
        .set_enabled(&harness.package, true)
        .await
        .expect("enable external package");
    assert!(
        ExternalSocketPackageProvider::resolve(&*harness.registry, &harness.package)
            .expect("online package resolve")
            .is_some()
    );

    let NetworkHarness {
        server,
        registry,
        peer,
        package,
    } = harness;
    peer.close().await;
    wait_until_offline(&registry, &package).await;

    let failure = ExternalSocketPackageProvider::resolve(&*registry, &package)
        .expect_err("disconnected package must fail closed");
    assert_eq!(failure.view_model.code, "EXTERNAL_PACKAGE_OFFLINE");
    server.shutdown().await;
}

#[tokio::test]
async fn real_websocket_rejects_a_non_packages_path_without_registering() {
    let address = reserve_loopback_address().await;
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::new(
        SqliteStore::in_memory().expect("in-memory registry"),
    )));
    let server = ExternalPackageServer::start(
        ExternalPackageServerConfig {
            bind_address: address,
            connection: ExternalPackageConnectionConfig::default(),
        },
        Arc::clone(&registry),
        Arc::new(EmptyUsage),
        Arc::new(UnusedListenerRuntime),
    )
    .await;

    let rejected = timeout(
        TEST_TIMEOUT,
        connect_async(format!("ws://{address}/not-packages")),
    )
    .await
    .expect("wrong-path handshake must terminate");

    assert!(rejected.is_err());
    assert!(
        registry
            .list()
            .await
            .expect("external package list")
            .is_empty()
    );
    server.shutdown().await;
}

async fn reserve_loopback_address() -> SocketAddr {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("reserve loopback port");
    let address = listener.local_addr().expect("reserved loopback address");
    drop(listener);
    address
}

fn spawn_external_peer(
    address: SocketAddr,
    registration: intercept_proxy_domain::ExternalPackageRegistration,
) -> ExternalPeer {
    let registration_count = Arc::new(AtomicUsize::new(0));
    let methods = Arc::new(Mutex::new(Vec::new()));
    let (close_tx, mut close_rx) = oneshot::channel();
    let task_registration_count = Arc::clone(&registration_count);
    let task_methods = Arc::clone(&methods);
    let task = tokio::spawn(async move {
        let (mut socket, response) = timeout(
            TEST_TIMEOUT,
            connect_async(format!("ws://{address}/packages")),
        )
        .await
        .expect("external peer connection deadline")
        .expect("external peer WebSocket connection");
        assert_eq!(response.status(), 101);
        loop {
            tokio::select! {
                _ = &mut close_rx => {
                    socket.close(None).await.expect("external peer close frame");
                    break;
                }
                message = socket.next() => {
                    let Some(message) = message else { break };
                    match message.expect("valid Proxy WebSocket frame") {
                        Message::Text(text) => {
                            respond_to_proxy_request(
                                &mut socket,
                                &registration,
                                &task_registration_count,
                                &task_methods,
                                &text,
                            ).await;
                        }
                        Message::Ping(payload) => {
                            socket.send(Message::Pong(payload)).await.expect("heartbeat pong");
                        }
                        Message::Close(_) => break,
                        Message::Pong(_) => {}
                        other => panic!("unexpected Proxy WebSocket frame: {other:?}"),
                    }
                }
            }
        }
    });
    ExternalPeer {
        registration_count,
        methods,
        close: Some(close_tx),
        task,
    }
}

async fn respond_to_proxy_request<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    registration: &intercept_proxy_domain::ExternalPackageRegistration,
    registration_count: &AtomicUsize,
    methods: &Mutex<Vec<String>>,
    text: &str,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let request: Value = serde_json::from_str(text).expect("Proxy JSON-RPC request");
    let method = request["method"].as_str().expect("JSON-RPC method");
    let result = match method {
        "package.register" => {
            registration_count.fetch_add(1, Ordering::AcqRel);
            serde_json::to_value(registration).expect("serialize registration")
        }
        "hooks.upstream.split_frame" => {
            methods.lock().push(method.to_owned());
            let frame: ExternalFrameRequest =
                serde_json::from_value(request["params"].clone()).expect("frame request");
            assert!(frame.bytes().expect("frame bytes").len() >= 3);
            serde_json::to_value(ExternalFrameResult::Complete { consumed_bytes: 3 })
                .expect("frame result")
        }
        "hooks.upstream.decrypt_and_decode" => {
            methods.lock().push(method.to_owned());
            let decoded: ExternalDecodeRequest =
                serde_json::from_value(request["params"].clone()).expect("decode request");
            assert_eq!(decoded.bytes().expect("decode bytes").len(), 3);
            serde_json::to_value(ExternalDecodeResponse {
                document: serde_json::from_value(json!({
                    "message_type": {"type": "string", "value": "0200"}
                }))
                .expect("external document"),
            })
            .expect("decode response")
        }
        "hooks.upstream.encode_and_encrypt" => {
            methods.lock().push(method.to_owned());
            let encoded: ExternalEncodeRequest =
                serde_json::from_value(request["params"].clone()).expect("encode request");
            assert_eq!(
                serde_json::to_value(encoded.document).expect("encoded document")["amount"],
                json!({"type": "int", "value": "42"})
            );
            serde_json::to_value(ExternalEncodeResponse::from_bytes(b"encoded"))
                .expect("encode response")
        }
        "document.upstream.render_message" => {
            methods.lock().push(method.to_owned());
            serde_json::to_value(ExternalDisplayResponse {
                html: "<p>ok</p>".to_owned(),
            })
            .expect("display response")
        }
        other => panic!("unexpected Proxy method: {other}"),
    };
    socket
        .send(Message::Text(
            json!({"jsonrpc": "2.0", "id": request["id"], "result": result})
                .to_string()
                .into(),
        ))
        .await
        .expect("external peer JSON-RPC response");
}

async fn wait_until_online(
    registry: &ExternalPackageRegistryAdapter,
    package: &ProtocolPackageRef,
) {
    timeout(TEST_TIMEOUT, async {
        loop {
            if registry
                .get(package)
                .await
                .expect("external package state")
                .is_some_and(|version| version.source.external_online() == Some(true))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("external package must become online before deadline");
}

async fn wait_until_offline(
    registry: &ExternalPackageRegistryAdapter,
    package: &ProtocolPackageRef,
) {
    timeout(TEST_TIMEOUT, async {
        loop {
            if registry
                .get(package)
                .await
                .expect("external package state")
                .is_some_and(|version| version.source.external_online() == Some(false))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("external package must become offline before deadline");
}

async fn wait_for_method_count(methods: &Mutex<Vec<String>>, expected: usize) {
    timeout(TEST_TIMEOUT, async {
        loop {
            if methods.lock().len() >= expected {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("external method calls must complete before deadline");
}

fn method_count(methods: &[String], expected: &str) -> usize {
    methods
        .iter()
        .filter(|method| method.as_str() == expected)
        .count()
}
