//! `LocalResponder` 的真实 TCP 集成测试。
//!
//! Local 配置类型不含 upstream；测试同时验证它可以独立应答、审计证据不会伪造上游，
//! 以及 processor 在写入前失败时不会泄漏部分响应。

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use super::{
    super::{
        SocketConnectionEvent, SocketConnectionTarget, SocketOpenedEvidence,
        SocketProcessingFailureKind, SocketRelayDirection, SocketRelayService, SocketRelayStage,
    },
    support::{
        LocalFactory, ProcessorOutcome, TestObserver, connect_retry, limits, local_config,
        reserve_address,
    },
};

#[tokio::test]
async fn local_raw_server_echoes_each_app_read_without_an_upstream() {
    let bind_addr = reserve_address();
    let observer = Arc::new(TestObserver::default());
    let mut config = local_config(bind_addr);
    config.read_chunk_bytes = 3;
    let service = Arc::new(
        SocketRelayService::build_local_raw_responder_with_observer(config, observer.clone())
            .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    let request = b"raw-local-echo";
    client.write_all(request).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, request);

    observer
        .wait_until(|event| matches!(event, SocketConnectionEvent::Closed { .. }))
        .await;
    cancellation.cancel();
    server.await.unwrap().unwrap();

    let events = observer.events();
    assert!(events.iter().any(|event| matches!(
        event,
        SocketConnectionEvent::Opened {
            evidence: SocketOpenedEvidence::LocalResponder {
                downstream_tls_peer: None
            },
            ..
        }
    )));
    let metrics = service.metrics().await;
    assert_eq!(metrics.client_to_server_read_bytes, request.len() as u64);
    assert_eq!(metrics.server_to_client_bytes, request.len() as u64);
}

#[tokio::test]
async fn local_server_rejects_pipelined_requests_under_strict_exchange_ordering() {
    let bind_addr = reserve_address();
    let factory = Arc::new(LocalFactory::new(ProcessorOutcome::Transform));
    let service = Arc::new(
        SocketRelayService::build_local_responder(
            local_config(bind_addr),
            factory.clone(),
            limits(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    client
        .write_all(&[1, b'a', 2, b'b', b'b', 1, b'c'])
        .await
        .unwrap();
    client.shutdown().await.unwrap();
    let mut responses = Vec::new();
    client.read_to_end(&mut responses).await.unwrap();
    assert!(responses.is_empty());
    assert_eq!(factory.created(), 2);

    cancellation.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn local_responder_observer_contains_no_upstream_evidence() {
    let bind_addr = reserve_address();
    let observer = Arc::new(TestObserver::default());
    let service = Arc::new(
        SocketRelayService::build_local_responder_with_observer(
            local_config(bind_addr),
            Arc::new(LocalFactory::new(ProcessorOutcome::Transform)),
            limits(),
            observer.clone(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    client.write_all(&[1, b'a']).await.unwrap();
    client.shutdown().await.unwrap();
    client.read_to_end(&mut Vec::new()).await.unwrap();
    observer
        .wait_until(|event| matches!(event, SocketConnectionEvent::Closed { .. }))
        .await;
    cancellation.cancel();
    server.await.unwrap().unwrap();

    let events = observer.events();
    assert!(events.iter().any(|event| matches!(
        event,
        SocketConnectionEvent::Admitted {
            target: SocketConnectionTarget::LocalResponder,
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        SocketConnectionEvent::Opened {
            evidence: SocketOpenedEvidence::LocalResponder {
                downstream_tls_peer: None
            },
            ..
        }
    )));
}

#[tokio::test]
async fn local_responder_records_responses_as_server_to_client_bytes() {
    let bind_addr = reserve_address();
    let service = Arc::new(
        SocketRelayService::build_local_responder(
            local_config(bind_addr),
            Arc::new(LocalFactory::new(ProcessorOutcome::Transform)),
            limits(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    client.write_all(&[2, b'o', b'k']).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, &[2, b'o', b'k']);

    cancellation.cancel();
    server.await.unwrap().unwrap();
    let metrics = service.metrics().await;
    assert_eq!(metrics.client_to_server_read_bytes, 3);
    assert_eq!(metrics.client_to_server_bytes, 0);
    assert_eq!(metrics.server_to_client_read_bytes, 0);
    assert_eq!(metrics.server_to_client_bytes, 3);
}

#[tokio::test]
async fn local_responder_rejects_upstream_probe_but_still_serves_locally() {
    let bind_addr = reserve_address();
    let service = Arc::new(
        SocketRelayService::build_local_responder(
            local_config(bind_addr),
            Arc::new(LocalFactory::new(ProcessorOutcome::Transform)),
            limits(),
        )
        .unwrap(),
    );
    assert_eq!(
        service.test_upstream_connection().await.unwrap_err().code,
        "CONFIG_INVALID"
    );

    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });
    let mut client = connect_retry(bind_addr).await;
    client.write_all(&[1, b'x']).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, &[1, b'x']);

    cancellation.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn local_processor_failure_writes_nothing_and_emits_one_stable_terminal() {
    assert_local_failure(
        ProcessorOutcome::Fail,
        SocketProcessingFailureKind::ProcessingFailed.as_str(),
    )
    .await;
}

#[tokio::test]
async fn local_processor_panic_writes_nothing_and_emits_one_stable_terminal() {
    assert_local_failure(
        ProcessorOutcome::Panic,
        SocketProcessingFailureKind::ProcessorPanicked.as_str(),
    )
    .await;
}

#[tokio::test]
async fn local_stop_while_reading_maps_to_cancelled_and_emits_one_terminal() {
    let bind_addr = reserve_address();
    let observer = Arc::new(TestObserver::default());
    let service = Arc::new(
        SocketRelayService::build_local_responder_with_observer(
            local_config(bind_addr),
            Arc::new(LocalFactory::new(ProcessorOutcome::Transform)),
            limits(),
            observer.clone(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    observer
        .wait_until(|event| matches!(event, SocketConnectionEvent::Opened { .. }))
        .await;
    cancellation.cancel();
    server.await.unwrap().unwrap();
    client.read_to_end(&mut Vec::new()).await.unwrap();

    let failures = observer
        .events()
        .into_iter()
        .filter_map(|event| match event {
            SocketConnectionEvent::Closed { failure, .. } => failure,
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].stage, SocketRelayStage::Shutdown);
    assert_eq!(
        failures[0].direction,
        Some(SocketRelayDirection::ClientToServer)
    );
    assert_eq!(failures[0].code, "SOCKET_RELAY_CANCELLED");
}

async fn assert_local_failure(outcome: ProcessorOutcome, expected_code: &'static str) {
    let bind_addr = reserve_address();
    let observer = Arc::new(TestObserver::default());
    let service = Arc::new(
        SocketRelayService::build_local_responder_with_observer(
            local_config(bind_addr),
            Arc::new(LocalFactory::new(outcome)),
            limits(),
            observer.clone(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let mut client = connect_retry(bind_addr).await;
    client.write_all(&[1, b'x']).await.unwrap();
    client.shutdown().await.unwrap();
    let mut output = Vec::new();
    client.read_to_end(&mut output).await.unwrap();
    assert!(output.is_empty());
    observer
        .wait_until(|event| matches!(event, SocketConnectionEvent::Closed { .. }))
        .await;
    cancellation.cancel();
    server.await.unwrap().unwrap();

    let closed = observer
        .events()
        .into_iter()
        .filter_map(|event| match event {
            SocketConnectionEvent::Closed { failure, bytes, .. } => Some((failure, bytes)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].1.server_to_client, 0);
    let failure = closed[0].0.expect("processor failure must be observable");
    assert_eq!(failure.stage, SocketRelayStage::FrameProcess);
    assert_eq!(
        failure.direction,
        Some(SocketRelayDirection::ClientToServer)
    );
    assert_eq!(failure.code, expected_code);
}

#[path = "local_responder/tls.rs"]
mod tls;
