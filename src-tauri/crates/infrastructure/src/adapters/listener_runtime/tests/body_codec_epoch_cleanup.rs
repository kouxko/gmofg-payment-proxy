#[tokio::test(flavor = "current_thread")]
async fn pending_start_failure_waits_for_stopping_owner_before_epoch_cleanup() {
    let first = available_http_listener("stopping owner").await;
    let mut workspace = ProxyWorkspace {
        listeners: vec![first.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let (runtime, resolver, pipeline) = runtime_with_recording_pipeline();
    runtime.start(workspace.clone(), first.clone()).await.unwrap();
    let old_epoch = runtime.runtime_epochs.read()[&workspace.id];

    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    let second = ProxyListener {
        id: ListenerId::new(),
        bind_address: address.ip().to_string(),
        port: address.port(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings::default()),
        ..ProxyListener::default()
    };
    workspace.listeners.push(second.clone());
    workspace.validate().unwrap();
    let (start_reached, start_release, _start_completed) =
        runtime.install_start_barrier(second.id).await;
    let start_reached_wait = start_reached.notified();
    let starting = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let workspace = workspace.clone();
        async move { runtime.start(workspace, second).await }
    });
    start_reached_wait.await;
    let (stop_reached, stop_release, _stop_completed) =
        runtime.install_stop_barrier(first.id).await;
    let stop_reached_wait = stop_reached.notified();
    let stopping = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        async move { runtime.stop(first.id).await }
    });
    stop_reached_wait.await;

    start_release.notify_one();
    starting.await.unwrap().expect_err("reserved port must fail");
    assert!(pipeline.snapshot().is_empty());
    assert!(runtime.runtime_epochs.read().get(&workspace.id).is_none());

    stop_release.notify_one();
    stopping.await.unwrap().unwrap();
    assert_eq!(pipeline.snapshot(), vec![old_epoch]);
    assert!(
        resolver
            .resolve(
                &response_context(old_epoch, first.id),
                MessageStage::Response,
                &empty_response(),
            )
            .is_err()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn last_pending_failure_cleans_epoch_once_after_stopping_owner_finished() {
    let first = available_http_listener("finished owner").await;
    let mut workspace = ProxyWorkspace {
        listeners: vec![first.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let (runtime, resolver, pipeline) = runtime_with_recording_pipeline();
    runtime.start(workspace.clone(), first.clone()).await.unwrap();
    let old_epoch = runtime.runtime_epochs.read()[&workspace.id];

    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    let second = ProxyListener {
        id: ListenerId::new(),
        bind_address: address.ip().to_string(),
        port: address.port(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings::default()),
        ..ProxyListener::default()
    };
    workspace.listeners.push(second.clone());
    workspace.validate().unwrap();
    let (start_reached, start_release, _start_completed) =
        runtime.install_start_barrier(second.id).await;
    let start_reached_wait = start_reached.notified();
    let starting = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let workspace = workspace.clone();
        async move { runtime.start(workspace, second).await }
    });
    start_reached_wait.await;

    runtime.stop(first.id).await.unwrap();
    assert!(pipeline.snapshot().is_empty());
    assert_eq!(runtime.runtime_epochs.read()[&workspace.id], old_epoch);
    start_release.notify_one();
    starting.await.unwrap().expect_err("reserved port must fail");

    assert_eq!(pipeline.snapshot(), vec![old_epoch]);
    assert!(runtime.runtime_epochs.read().get(&workspace.id).is_none());
    assert!(
        resolver
            .resolve(
                &response_context(old_epoch, first.id),
                MessageStage::Response,
                &empty_response(),
            )
            .is_err()
    );
}
