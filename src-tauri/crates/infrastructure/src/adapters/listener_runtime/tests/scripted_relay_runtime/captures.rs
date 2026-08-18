//! Relay 两方向正式 Frame capture 的真实 TCP 证据。

use std::time::Duration;

use intercept_proxy_application::{
    PageRequest, SocketCaptureDocumentValue, SocketCaptureInteger, SocketCapturePayload,
    SocketCaptureQuery, SocketCaptureSort, SocketDisplayResult, SocketWriteKind, SortDirection,
};

use super::*;

pub(super) fn query() -> SocketCaptureQuery {
    SocketCaptureQuery {
        workspace_id: None,
        listener_id: None,
        session_id: None,
        connection_id: None,
        package: None,
        direction: None,
        kind: None,
        occurred_from: None,
        occurred_to: None,
        sort: SocketCaptureSort::OccurredAt,
        direction_sort: SortDirection::Asc,
        page: PageRequest {
            page: 1,
            page_size: 20,
        },
    }
}

async fn wait_for_two(
    repository: &crate::adapters::SocketCaptureRepositoryAdapter,
) -> intercept_proxy_application::SocketCapturePageViewModel {
    for _ in 0..100 {
        let page = repository.query(&query()).unwrap();
        if page.rows.len() == 2 {
            return page;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("relay capture drain did not persist both directions");
}

#[tokio::test]
async fn relay_records_each_direction_only_after_commit_with_exact_documents_and_rules() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = reserve_port().await;
    let state = State {
        decode: true,
        encode: true,
    };
    let listener = listener(listener_port, upstream_port, state, state);
    let workspace = workspace(&listener, state, state);
    let upstream_rule = workspace
        .socket_rules
        .iter()
        .find(|rule| rule.direction() == SocketDirection::Upstream)
        .unwrap()
        .rule_id();
    let downstream_rule = workspace
        .socket_rules
        .iter()
        .find(|rule| rule.direction() == SocketDirection::Downstream)
        .unwrap()
        .rule_id();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = [0_u8; 2];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(request, [161, 42]);
        stream.write_all(&[2, 22]).await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let packages = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    packages.install_zip(&package_zip()).unwrap();
    packages.set_enabled(&package(), true).unwrap();
    let captures = Arc::new(crate::adapters::SocketCaptureRepositoryAdapter::new(
        Arc::clone(&store),
    ));
    let runtime = test_listener_runtime_with_packages(store, packages);
    runtime.set_socket_capture_repository(Arc::clone(&captures));
    runtime.start(workspace, listener.clone()).await.unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    client.write_all(&[2]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(captures.query(&query()).unwrap().total, 0);
    client.write_all(&[11]).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, [209, 42]);
    upstream_task.await.unwrap();

    let page = wait_for_two(&captures).await;
    let mut records = Vec::new();
    for row in page.rows {
        records.push(captures.get_detail(row.capture_id).unwrap().record);
    }
    let upstream = records
        .iter()
        .find_map(|record| match &record.payload {
            SocketCapturePayload::RelayFrame(frame)
                if frame.direction == SocketDirection::Upstream =>
            {
                Some(frame)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(upstream.origin, [2, 11]);
    assert_eq!(upstream.written, [161, 42]);
    assert_eq!(
        upstream.document.as_ref().unwrap().get("amount").unwrap(),
        &SocketCaptureDocumentValue::Int(SocketCaptureInteger::from_i64(42))
    );
    assert_eq!(upstream.matched_rule_ids, [upstream_rule]);
    assert_eq!(upstream.write_kind, SocketWriteKind::Encoded);
    assert_eq!(
        upstream.display,
        SocketDisplayResult::UntrustedHtml {
            html: "<p>runtime</p>".to_owned()
        }
    );
    let downstream = records
        .iter()
        .find_map(|record| match &record.payload {
            SocketCapturePayload::RelayFrame(frame)
                if frame.direction == SocketDirection::Downstream =>
            {
                Some(frame)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(downstream.origin, [2, 22]);
    assert_eq!(downstream.written, [209, 42]);
    assert_eq!(downstream.matched_rule_ids, [downstream_rule]);
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn clear_after_output_commit_cannot_revive_a_blocked_relay_capture() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = reserve_port().await;
    let state = State {
        decode: true,
        encode: true,
    };
    let listener = listener(listener_port, upstream_port, state, state);
    let workspace = workspace(&listener, state, state);
    let workspace_id = workspace.id;
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = [0_u8; 2];
        stream.read_exact(&mut request).await.unwrap();
        stream.write_all(&[2, 22]).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let packages = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    packages.install_zip(&package_zip()).unwrap();
    packages.set_enabled(&package(), true).unwrap();
    let captures = Arc::new(crate::adapters::SocketCaptureRepositoryAdapter::new(
        Arc::clone(&store),
    ));
    let runtime = test_listener_runtime_with_packages(store, packages);
    runtime.set_socket_capture_repository(Arc::clone(&captures));
    runtime.start(workspace, listener.clone()).await.unwrap();
    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    runtime.block_next_socket_capture_display_for_test(entered_sender, release_receiver);

    let client = tokio::spawn(async move {
        let mut client = TcpStream::connect(("127.0.0.1", listener_port))
            .await
            .unwrap();
        client.write_all(&[2, 11]).await.unwrap();
        client.shutdown().await.unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        response
    });
    tokio::task::spawn_blocking(move || entered_receiver.recv_timeout(Duration::from_secs(2)))
        .await
        .unwrap()
        .expect("Display worker must stop after output_committed took its ticket");
    assert_eq!(captures.clear_completed(workspace_id).unwrap(), 0);
    release_sender.send(()).unwrap();
    assert_eq!(client.await.unwrap(), [209, 42]);
    upstream_task.await.unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;
    for row in captures.query(&query()).unwrap().rows {
        let record = captures.get_detail(row.capture_id).unwrap().record;
        let SocketCapturePayload::RelayFrame(frame) = record.payload else {
            panic!("expected RelayFrame")
        };
        assert_ne!(frame.direction, SocketDirection::Upstream);
    }
    runtime.stop(listener.id).await.unwrap();
}
