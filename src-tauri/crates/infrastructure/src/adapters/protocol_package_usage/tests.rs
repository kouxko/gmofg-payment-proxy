use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, InMemoryListenerRuntime, InMemoryWorkspaceStore, ListenerRuntimePort,
    ListenerRuntimeState, ListenerStatusViewModel, ListenerUpstreamConnectionTestViewModel,
    ListenerUpstreamTlsTestViewModel, ProtocolPackageUsageQueryPort, ProxyListener, ProxyWorkspace,
    WorkspaceRepositoryPort,
};
use intercept_proxy_domain::{
    DirectionProcessingOptions, ListenerDataPlane, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, ScriptedSocketProcessing, SocketEndpoint, SocketPayloadProcessing,
    SocketRelaySecurity, SocketRelaySettings,
};

use super::ProtocolPackageUsageQueryAdapter;

#[tokio::test]
async fn usages_match_exact_identity_across_workspaces_and_merge_runtime_state() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let target = package("example-protocol", "1.0.0");
    let other_version = package("example-protocol", "2.0.0");
    let other_id = package("other-protocol", "1.0.0");

    let first = workspaces
        .import_workspace(workspace(
            "Alpha",
            vec![
                scripted_listener("alpha-target", 18_101, target.clone()),
                scripted_listener("alpha-other-version", 18_102, other_version),
                ProxyListener {
                    name: "alpha-http".into(),
                    port: 18_103,
                    ..ProxyListener::default()
                },
            ],
        ))
        .await
        .unwrap();
    let second = workspaces
        .import_workspace(workspace(
            "Beta",
            vec![
                scripted_listener("beta-target", 18_201, target.clone()),
                scripted_listener("beta-other-id", 18_202, other_id),
            ],
        ))
        .await
        .unwrap();

    let runtime = Arc::new(InMemoryListenerRuntime::default());
    runtime
        .start(first.clone(), first.listeners[0].clone())
        .await
        .unwrap();
    let adapter = ProtocolPackageUsageQueryAdapter::new(
        Arc::<InMemoryWorkspaceStore>::clone(&workspaces),
        Arc::<InMemoryListenerRuntime>::clone(&runtime),
    );

    let usages = adapter.usages(&target).await.unwrap();

    assert_eq!(usages.len(), 2);
    assert!(
        usages
            .windows(2)
            .all(|pair| (pair[0].workspace_id, pair[0].listener_id)
                < (pair[1].workspace_id, pair[1].listener_id)),
        "adapter output must remain deterministic"
    );
    let running = usages
        .iter()
        .find(|usage| usage.workspace_id == first.id)
        .unwrap();
    assert_eq!(running.workspace_name, "Alpha");
    assert_eq!(running.listener_name, "alpha-target");
    assert_eq!(running.runtime_state, ListenerRuntimeState::Running);

    let stopped = usages
        .iter()
        .find(|usage| usage.workspace_id == second.id)
        .unwrap();
    assert_eq!(stopped.workspace_name, "Beta");
    assert_eq!(stopped.listener_name, "beta-target");
    assert_eq!(stopped.runtime_state, ListenerRuntimeState::Stopped);

    let counts = adapter.usage_counts().await.unwrap();
    assert_eq!(counts.len(), 3);
    let target_count = counts.iter().find(|count| count.package == target).unwrap();
    assert_eq!(target_count.reference_count, 2);
    assert_eq!(target_count.active_reference_count, 1);
    assert!(counts.windows(2).all(|pair| {
        (&pair[0].package.id, &pair[0].package.version)
            < (&pair[1].package.id, &pair[1].package.version)
    }));
}

#[tokio::test]
async fn runtime_snapshot_failure_is_not_represented_as_no_usage() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let target = package("example-protocol", "1.0.0");
    workspaces
        .import_workspace(workspace(
            "Referenced",
            vec![scripted_listener("target", 18_301, target.clone())],
        ))
        .await
        .unwrap();
    let adapter = ProtocolPackageUsageQueryAdapter::new(workspaces, Arc::new(FailingRuntime));

    let error = adapter.usages(&target).await.unwrap_err();

    assert_eq!(error.view_model.code, "RUNTIME_SNAPSHOT_UNAVAILABLE");
    assert_eq!(
        adapter.usage_counts().await.unwrap_err().view_model.code,
        "RUNTIME_SNAPSHOT_UNAVAILABLE"
    );
}

fn workspace(name: &str, listeners: Vec<ProxyListener>) -> ProxyWorkspace {
    ProxyWorkspace {
        name: name.into(),
        listeners,
        ..ProxyWorkspace::default()
    }
}

fn scripted_listener(name: &str, port: u16, package: ProtocolPackageRef) -> ProxyListener {
    ProxyListener {
        name: name.into(),
        enabled: true,
        port,
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings::relay(
            SocketEndpoint {
                host: "upstream.example.test".into(),
                port: 9_001,
            },
            SocketRelaySecurity::Transparent,
            intercept_proxy_domain::DEFAULT_SOCKET_MAXIMUM_CONNECTIONS,
            SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package,
                upstream: DirectionProcessingOptions::default(),
                downstream: DirectionProcessingOptions::default(),
            }),
        )),
        ..ProxyListener::default()
    }
}

fn package(id: &str, version: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(id).unwrap(),
        version: ProtocolPackageVersion::new(version).unwrap(),
    }
}

#[derive(Debug)]
struct FailingRuntime;

fn runtime_failure<T>() -> AppResult<T> {
    Err(AppError::new(
        "RUNTIME_SNAPSHOT_UNAVAILABLE",
        "test runtime snapshot failure",
    ))
}

#[async_trait]
impl ListenerRuntimePort for FailingRuntime {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        runtime_failure()
    }

    async fn start(
        &self,
        _: ProxyWorkspace,
        _: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel> {
        runtime_failure()
    }

    async fn stop(
        &self,
        _: intercept_proxy_domain::ListenerId,
    ) -> AppResult<ListenerStatusViewModel> {
        runtime_failure()
    }

    async fn test_upstream_connection(
        &self,
        _: ProxyWorkspace,
        _: ProxyListener,
    ) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
        runtime_failure()
    }

    async fn test_upstream_tls(
        &self,
        _: ProxyWorkspace,
        _: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        runtime_failure()
    }
}
