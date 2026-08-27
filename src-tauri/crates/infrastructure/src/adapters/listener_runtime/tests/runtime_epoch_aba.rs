use super::*;

#[tokio::test(flavor = "current_thread")]
async fn start_stop_start_assigns_a_new_runtime_epoch() {
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let listener = ProxyListener {
        name: "runtime epoch ABA".into(),
        bind_address: address.ip().to_string(),
        port: address.port(),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));

    runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();
    let first = runtime.runtime_epochs.read()[&workspace.id];
    runtime.stop(listener.id).await.unwrap();
    runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();
    let second = runtime.runtime_epochs.read()[&workspace.id];

    assert_ne!(first, second);
    runtime.stop(listener.id).await.unwrap();
}
