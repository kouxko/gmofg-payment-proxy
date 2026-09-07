use crate::adapters::{
    EnvironmentApplyLeaseResourceKey, EnvironmentApplyResourceGateRegistry,
    WorkspaceRepositoryAdapter,
};
use intercept_proxy_application::WorkspaceRepositoryPort;

#[tokio::test(flavor = "current_thread")]
async fn replace_rule_definitions_waits_for_the_listener_apply_gate() {
    use intercept_proxy_application::ListenerRuntimePort;

    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let listener = ProxyListener::default();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let workspaces = WorkspaceRepositoryAdapter::new(store.clone());
    let mut workspace = workspaces.create("listener gate".into()).await.unwrap();
    workspace.listeners = vec![listener.clone()];
    let workspace = workspaces.save(workspace).await.unwrap();
    let runtime = test_listener_runtime(store.clone())
        .with_environment_apply_resource_gates(gates.clone());
    let guard = gates
        .acquire(EnvironmentApplyLeaseResourceKey::Listener(
            listener.id.as_uuid(),
        ))
        .await;

    let mut replacement = Box::pin(runtime.replace_rule_definitions(
        &workspaces,
        workspace,
        listener.id,
    ));
    assert!(matches!(
        std::future::poll_fn(|context| {
            std::task::Poll::Ready(replacement.as_mut().poll(context))
        })
        .await,
        std::task::Poll::Pending
    ));

    drop(guard);
    replacement.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn stopped_baseline_rejects_start_published_before_rule_save() {
    use intercept_proxy_application::{ListenerRuntimePort, WorkspaceRepositoryPort};

    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let listener = ProxyListener {
        bind_address: address.ip().to_string(),
        port: address.port(),
        ..ProxyListener::default()
    };
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let workspaces = Arc::new(WorkspaceRepositoryAdapter::new(store.clone()));
    let mut workspace = workspaces.create("start race".into()).await.unwrap();
    workspace.listeners = vec![listener.clone()];
    let workspace = workspaces.save(workspace).await.unwrap();
    let baseline_revision = workspace.revision;
    let runtime = Arc::new(test_listener_runtime(store));
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
    let (reached, release, completed) = runtime.install_activation_barrier(listener.id).await;
    let starting = tokio::spawn({
        let runtime = runtime.clone();
        let workspace = workspace.clone();
        let listener = listener.clone();
        async move { runtime.start(workspace, listener).await }
    });
    reached.notified().await;
    let replacing = tokio::spawn({
        let runtime = runtime.clone();
        let workspaces = workspaces.clone();
        let workspace = workspace.clone();
        async move {
            runtime
                .replace_rule_definitions(workspaces.as_ref(), workspace, listener.id)
                .await
        }
    });

    release.notify_one();
    completed.notified().await;
    starting.await.unwrap().unwrap();
    let error = replacing.await.unwrap().unwrap_err();
    assert_eq!(error.view_model.code, "REVISION_CONFLICT");
    assert_eq!(
        workspaces.get(workspace.id).await.unwrap().revision,
        baseline_revision
    );
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_stop_keeps_the_listener_apply_gate_until_detached_cleanup_finishes() {
    use intercept_proxy_application::ListenerRuntimePort;

    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let listener = ProxyListener {
        bind_address: address.ip().to_string(),
        port: address.port(),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let runtime = Arc::new(
        test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()))
            .with_environment_apply_resource_gates(gates.clone()),
    );
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
    runtime.start(workspace, listener.clone()).await.unwrap();
    let (reached, release, completed) = runtime.install_stop_barrier(listener.id).await;
    let caller = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.stop(listener.id).await }
    });
    reached.notified().await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());

    let key = EnvironmentApplyLeaseResourceKey::Listener(listener.id.as_uuid());
    let mut competing = Box::pin(gates.acquire(key));
    assert!(matches!(
        std::future::poll_fn(|context| {
            std::task::Poll::Ready(competing.as_mut().poll(context))
        })
        .await,
        std::task::Poll::Pending
    ));

    release.notify_one();
    completed.notified().await;
    drop(competing.await);
}
