#[tokio::test(flavor = "current_thread")]
async fn aborted_start_caller_releases_pending_epoch_and_allows_restart() {
    let listener = available_http_listener("cancelled start").await;
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let (runtime, resolver, pipeline) = runtime_with_recording_pipeline();
    let (reached, _release, completed) = runtime.install_start_barrier(listener.id).await;
    let reached_wait = reached.notified();
    let completed_wait = completed.notified();
    let caller = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let workspace = workspace.clone();
        let listener = listener.clone();
        async move { runtime.start(workspace, listener).await }
    });
    reached_wait.await;
    let reserved_epoch = runtime.runtime_epochs.read()[&workspace.id];

    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    completed_wait.await;

    assert!(!runtime.pending_starts.read().contains_key(&listener.id));
    assert!(runtime.runtime_epochs.read().get(&workspace.id).is_none());
    assert_eq!(pipeline.snapshot(), vec![reserved_epoch]);
    assert!(
        resolver
            .resolve(
                &response_context(reserved_epoch, listener.id),
                MessageStage::Response,
                &empty_response(),
            )
            .is_err()
    );
    runtime.start(workspace, listener.clone()).await.unwrap();
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn aborted_start_after_epoch_activation_retires_exact_epoch_before_restart() {
    let listener = available_http_listener("cancelled activated start").await;
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let workspace_id = workspace.id;
    let (runtime, _resolver, pipeline) = runtime_with_recording_pipeline();
    let (reached, _release, completed) = runtime.install_activation_barrier(listener.id).await;
    let reached_wait = reached.notified();
    let completed_wait = completed.notified();
    let caller = tokio::spawn({
        let runtime = runtime.clone();
        let workspace = workspace.clone();
        let listener = listener.clone();
        async move { runtime.start(workspace, listener).await }
    });
    reached_wait.await;
    let activated_epoch = runtime.runtime_epochs.read()[&workspace.id];
    assert_eq!(pipeline.started_snapshot(), vec![activated_epoch]);

    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    completed_wait.await;
    assert_eq!(pipeline.snapshot(), vec![activated_epoch]);
    assert!(!runtime.runtime_epochs.read().contains_key(&workspace.id));

    runtime.start(workspace, listener.clone()).await.unwrap();
    let restarted_epoch = runtime.runtime_epochs.read()[&workspace_id];
    assert_ne!(restarted_epoch, activated_epoch);
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn aborted_stop_caller_cannot_cancel_epoch_cleanup() {
    let listener = available_http_listener("cancelled stop").await;
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let (runtime, resolver, pipeline) = runtime_with_recording_pipeline();
    runtime.start(workspace.clone(), listener.clone()).await.unwrap();
    let old_epoch = runtime.runtime_epochs.read()[&workspace.id];
    let (reached, release, completed) = runtime.install_stop_barrier(listener.id).await;
    let reached_wait = reached.notified();
    let completed_wait = completed.notified();
    let listener_id = listener.id;
    let caller = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        async move { runtime.stop(listener_id).await }
    });
    reached_wait.await;

    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    release.notify_one();
    completed_wait.await;

    assert_eq!(pipeline.snapshot(), vec![old_epoch]);
    assert!(runtime.runtime_epochs.read().get(&workspace.id).is_none());
    assert!(
        resolver
            .resolve(
                &response_context(old_epoch, listener.id),
                MessageStage::Response,
                &empty_response(),
            )
            .is_err()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn aborted_stop_caller_only_cleans_exact_listener_while_sibling_runs() {
    let first = available_http_listener("cancelled sibling stop").await;
    let mut workspace = ProxyWorkspace {
        listeners: vec![first.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let (runtime, resolver, pipeline) = runtime_with_recording_pipeline();
    runtime.start(workspace.clone(), first.clone()).await.unwrap();
    let sibling = available_http_listener("surviving sibling").await;
    workspace.listeners.push(sibling.clone());
    workspace.validate().unwrap();
    runtime
        .start(workspace.clone(), sibling.clone())
        .await
        .unwrap();
    let shared_epoch = runtime.runtime_epochs.read()[&workspace.id];
    let (reached, release, completed) = runtime.install_stop_barrier(first.id).await;
    let reached_wait = reached.notified();
    let completed_wait = completed.notified();
    let first_id = first.id;
    let caller = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        async move { runtime.stop(first_id).await }
    });
    reached_wait.await;

    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    release.notify_one();
    completed_wait.await;

    assert!(pipeline.snapshot().is_empty());
    assert_eq!(runtime.runtime_epochs.read()[&workspace.id], shared_epoch);
    assert!(
        resolver
            .resolve(
                &response_context(shared_epoch, first.id),
                MessageStage::Response,
                &empty_response(),
            )
            .is_err()
    );
    assert_eq!(
        resolver
            .resolve(
                &response_context(shared_epoch, sibling.id),
                MessageStage::Response,
                &empty_response(),
            )
            .unwrap()
            .expect("sibling snapshot survives")
            .id(),
        "shift-jis"
    );
    runtime.stop(sibling.id).await.unwrap();
    assert_eq!(pipeline.snapshot(), vec![shared_epoch]);
}
