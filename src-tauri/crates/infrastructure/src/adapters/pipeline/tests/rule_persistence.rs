#[tokio::test(flavor = "current_thread")]
async fn request_and_response_wait_for_durable_sqlite_without_blocking_runtime_progress() {
    use std::{path::PathBuf, sync::mpsc, task::Poll};

    use intercept_proxy_application::ProxyWorkspace;
    use intercept_proxy_domain::{HttpListenerSettings, ListenerDataPlane, ListenerId, ProxyListener};
    use tokio::sync::oneshot;

    use crate::{InfrastructureError, SqliteExecutor, SqliteStore, WorkspaceRecord};
    use crate::adapters::{FileSelection, NativeFileDialog};

    #[derive(Debug)]
    struct RulePersistenceNoDialog;
    impl NativeFileDialog for RulePersistenceNoDialog {
        fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> { Ok(None) }
        fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> { Ok(None) }
    }

    async fn occupy(
        executor: SqliteExecutor,
    ) -> (
        mpsc::Sender<()>,
        tokio::task::JoinHandle<Result<(), InfrastructureError>>,
    ) {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let task = tokio::spawn(async move {
            executor.execute(move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, InfrastructureError>(())
            }).await
        });
        entered_rx.await.unwrap();
        (release_tx, task)
    }

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let executor = SqliteExecutor::new(Arc::clone(&store));
    let listener = ProxyListener {
        id: ListenerId::new(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings::default()),
        ..ProxyListener::default()
    };
    let mut request_rule = view_to_domain_rule(one_shot_delay_rule()).unwrap();
    request_rule.one_shot = false;
    request_rule.channel = Some(
        intercept_proxy_domain::ChannelId::new(listener.id.to_string()).unwrap(),
    );
    let mut response_rule = view_to_domain_rule(response_status_rule(503)).unwrap();
    response_rule.channel = Some(
        intercept_proxy_domain::ChannelId::new(listener.id.to_string()).unwrap(),
    );
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace
        .replace_http_runtime_rules(vec![request_rule, response_rule])
        .unwrap();
    let record = WorkspaceRecord {
        id: workspace.id.as_uuid(),
        revision: workspace.revision.get(),
        value: crate::adapters::common::encode_workspace_record(&workspace).unwrap(),
        updated_at: Utc::now(),
    };
    store.insert_workspace(&record).unwrap();
    store.select_workspace(record.id).unwrap();
    let rules = Arc::new(RuleRepositoryAdapter::new(
        (executor.clone(), Arc::clone(&store)),
        Arc::new(RulePersistenceNoDialog),
        &[],
    ));
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(), rules, Arc::new(InMemorySessionStore::default()),
        Arc::new(BreakpointCoordinator::default()), Arc::new(EventHub::new(16)),
        test_capture_repository(),
    );
    let context = test_context(
        Uuid::new_v4(), Uuid::new_v4(), ChannelId::new(listener.id.to_string()).unwrap(),
    );
    open_test_connection(&pipeline, &context).await;

    let (release, occupied) = occupy(executor.clone()).await;
    let mut request = request_message("body");
    let mut policy = Box::pin(pipeline.apply_request_policy(&context, &mut request));
    assert!(matches!(poll_body_codec_policy_once(policy.as_mut()).await, Poll::Pending));
    let (tick_tx, tick_rx) = oneshot::channel();
    tokio::spawn(async move { tick_tx.send(()).unwrap() });
    tick_rx.await.unwrap();
    release.send(()).unwrap();
    occupied.await.unwrap().unwrap();
    assert!(!policy.await.unwrap().is_empty());

    let (release, occupied) = occupy(executor).await;
    let mut response = response_message();
    let mut policy = Box::pin(pipeline.apply_response_policy(&context, &mut response));
    assert!(matches!(poll_body_codec_policy_once(policy.as_mut()).await, Poll::Pending));
    let (tick_tx, tick_rx) = oneshot::channel();
    tokio::spawn(async move { tick_tx.send(()).unwrap() });
    tick_rx.await.unwrap();
    release.send(()).unwrap();
    occupied.await.unwrap().unwrap();
    assert!(!policy.await.unwrap().is_empty());
}
