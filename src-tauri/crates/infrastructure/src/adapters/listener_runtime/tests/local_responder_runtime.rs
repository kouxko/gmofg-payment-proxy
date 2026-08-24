//! `LocalServer` 从 Listener 到两条协议 Flow 的生产接线测试。

use intercept_proxy_domain::{
    DocumentAction, DocumentFieldName, DocumentValue, ProtocolDirection,
    ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::*;
use support::*;

#[tokio::test]
async fn direct_local_responder_runs_the_core_raw_server_in_production_wiring() {
    let id = "local-raw-production";
    let port = reserve_port().await;
    let mut listener = local_listener(id, port);
    let ListenerDataPlane::Socket(socket) = &mut listener.data_plane else {
        unreachable!("local listener fixture must remain Socket")
    };
    socket.processing = intercept_proxy_domain::SocketPayloadProcessing::Direct;
    socket.runtime_limits.read_chunk_bytes = 3;
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    runtime
        .start(workspace(listener.clone(), Vec::new()), listener.clone())
        .await
        .unwrap();

    let request = b"direct-local-raw";
    assert_eq!(request_once(port, request).await, request);
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn local_server_echoes_upstream_payload_then_runs_downstream_pipeline() {
    let id = "local-server-flow";
    let port = reserve_port().await;
    let listener = local_listener(id, port);
    let rule = ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::new(),
        true,
        10,
        1,
        listener.id,
        package_ref(id),
        1,
        ProtocolDirection::Downstream,
        Vec::new(),
        vec![DocumentAction::SetField {
            field: DocumentFieldName::new("amount").unwrap(),
            value: DocumentValue::Int(42),
        }],
    )
    .unwrap();
    let runtime = start_local_runtime(
        id,
        BASIC_SCHEMA,
        BASIC_SCRIPT,
        workspace(listener.clone(), vec![rule]),
        &listener,
    )
    .await;

    assert_eq!(request_once(port, &[2, 11]).await, [2, 42]);
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn local_server_rejects_two_requests_in_one_inflight_window() {
    let id = "local-server-strict-order";
    let port = reserve_port().await;
    let listener = local_listener(id, port);
    let runtime = start_local_runtime(
        id,
        BASIC_SCHEMA,
        BASIC_SCRIPT,
        workspace(listener.clone(), Vec::new()),
        &listener,
    )
    .await;
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(&[2, 7, 2, 8]).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    let _ = client.read_to_end(&mut response).await;
    assert!(response.is_empty());
    runtime.stop(listener.id).await.unwrap();
}

#[path = "local_responder_runtime/support.rs"]
mod support;
