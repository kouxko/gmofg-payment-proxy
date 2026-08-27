use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use intercept_proxy_application::{
    AppResult, EventHub, ExternalPackageApplicationPort, ProtocolPackageUsageCount,
    ProtocolPackageUsageQueryPort, ProtocolPackageUsageViewModel,
};
use intercept_proxy_domain::ProtocolPackageRef;
use serde_json::Value;
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tokio_tungstenite::tungstenite::Message;

use super::*;

#[test]
fn accepted_connection_limit_is_fail_fast_observable_and_releases_capacity() {
    assert_eq!(MAX_ACCEPTED_CONNECTIONS, 256);
    let events = Arc::new(EventHub::new(8));
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(
        SqliteStore::in_memory().expect("in-memory store"),
    ));
    registry.set_event_hub(Arc::clone(&events));
    let admission = Arc::new(tokio::sync::Semaphore::new(1));
    let first = try_admit_connection(
        &admission,
        &registry,
        "127.0.0.1:49001".parse().expect("remote address"),
    )
    .expect("first accepted connection");
    assert!(
        try_admit_connection(
            &admission,
            &registry,
            "127.0.0.1:49002".parse().expect("remote address"),
        )
        .is_none()
    );
    assert!(events.diagnostic_log_snapshot().iter().any(|event| {
        event
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("EXTERNAL_PACKAGE_CONNECTION_LIMIT_REACHED"))
    }));
    drop(first);
    assert!(
        try_admit_connection(
            &admission,
            &registry,
            "127.0.0.1:49003".parse().expect("remote address"),
        )
        .is_some()
    );
}

#[tokio::test]
async fn rejected_handshake_releases_the_accepted_connection_permit() {
    let admission = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = Arc::clone(&admission)
        .try_acquire_owned()
        .expect("first accepted connection");
    assert!(Arc::clone(&admission).try_acquire_owned().is_err());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let connecting = TcpStream::connect(listener.local_addr().expect("address"));
    let (peer, accepted) = tokio::join!(connecting, listener.accept());
    let mut peer = peer.expect("test peer");
    let (stream, remote_address) = accepted.expect("accepted connection");
    peer.write_all(b"GET /wrong HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .expect("invalid handshake");
    peer.shutdown().await.expect("peer shutdown");
    handle_connection(
        stream,
        remote_address,
        1,
        ExternalPackageConnectionConfig::default(),
        ConnectionServices {
            registry: Arc::new(ExternalPackageRegistryAdapter::new(Arc::new(
                SqliteStore::in_memory().expect("in-memory store"),
            ))),
            usage: Arc::new(FixedUsage(Vec::new())),
            listener_runtime: Arc::new(TrackingRuntime::immediate()),
        },
        CancellationToken::new(),
        permit,
    )
    .await;
    assert_eq!(admission.available_permits(), 1);
}

#[derive(Debug)]
struct PackageScopedUsage {
    package: ProtocolPackageRef,
    usages: Vec<ProtocolPackageUsageViewModel>,
}

#[async_trait]
impl ProtocolPackageUsageQueryPort for PackageScopedUsage {
    async fn usages(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        Ok(if package == &self.package {
            self.usages.clone()
        } else {
            Vec::new()
        })
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn failed_package_cleanup_stops_only_its_listener_while_another_package_progresses() {
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::new(
        SqliteStore::in_memory().expect("in-memory store"),
    )));
    let failed_registration = registration_with_id("failed-package");
    let healthy_registration = registration_with_id("healthy-package");
    let failed_package = failed_registration.package().identity().clone();
    let healthy_package = healthy_registration.package().identity().clone();
    let (failed_client, _failed_peer) = connected_client(&failed_registration, 71).await;
    let (healthy_client, mut healthy_peer) = connected_client(&healthy_registration, 72).await;
    let failed = registry
        .accept_registration(
            &failed_registration,
            external_package_registration_fingerprint(&failed_registration).expect("fingerprint"),
            failed_client,
        )
        .await
        .expect("failed package accepted");
    registry
        .accept_registration(
            &healthy_registration,
            external_package_registration_fingerprint(&healthy_registration).expect("fingerprint"),
            healthy_client,
        )
        .await
        .expect("healthy package accepted");
    assert!(
        registry
            .mark_disconnected(&failed_package, failed.connection_id)
            .await
    );
    let failed_listener = ListenerId::new();
    let healthy_listener = ListenerId::new();
    let runtime = Arc::new(TrackingRuntime::immediate());
    stop_exact_package_listeners(
        &failed_package,
        failed.connection_id,
        registry.as_ref(),
        &PackageScopedUsage {
            package: failed_package.clone(),
            usages: vec![running_usage(failed_listener)],
        },
        runtime.as_ref(),
    )
    .await;
    assert_eq!(runtime.stopped.lock().as_slice(), &[failed_listener]);
    assert!(!runtime.stopped.lock().contains(&healthy_listener));

    let healthy_client = registry.client(&healthy_package).expect("healthy online");
    let call = tokio::spawn(async move {
        healthy_client
            .call::<_, Value>(
                "hooks.upstream.decode",
                &serde_json::json!({"healthy":true}),
            )
            .await
    });
    let request = loop {
        match healthy_peer.next().await.expect("request").expect("frame") {
            Message::Text(text) => break serde_json::from_str::<Value>(&text).expect("JSON"),
            Message::Ping(payload) => healthy_peer
                .send(Message::Pong(payload))
                .await
                .expect("pong"),
            other => panic!("unexpected frame: {other:?}"),
        }
    };
    healthy_peer
        .send(Message::Text(
            serde_json::json!({"jsonrpc":"2.0","id":request["id"],"result":{"ok":true}})
                .to_string()
                .into(),
        ))
        .await
        .expect("response");
    assert_eq!(
        call.await.expect("join").expect("healthy progress"),
        serde_json::json!({"ok":true})
    );
    registry
        .disconnect(&healthy_package)
        .await
        .expect("cleanup");
}
