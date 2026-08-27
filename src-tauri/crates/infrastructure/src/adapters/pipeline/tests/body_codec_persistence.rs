async fn poll_body_codec_policy_once<F: std::future::Future>(
    future: std::pin::Pin<&mut F>,
) -> std::task::Poll<F::Output> {
    let mut future = future;
    std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx))).await
}

#[tokio::test(flavor = "current_thread")]
async fn frozen_body_codec_pipeline_progresses_while_sqlite_executor_is_occupied() {
    use std::sync::mpsc;

    use intercept_proxy_domain::{
        BodyCodecKind, HttpListenerSettings, ListenerDataPlane, ListenerId, ProxyListener,
    };
    use tokio::sync::oneshot;

    use crate::{InfrastructureError, SqliteExecutor, SqliteStore, adapters::WorkspaceBodyCodecResolver};

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let executor = SqliteExecutor::new(store);
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let occupied = tokio::spawn(async move {
        executor
            .execute(move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, InfrastructureError>(())
            })
            .await
    });
    entered_rx.await.unwrap();

    let listener = ProxyListener {
        id: ListenerId::new(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            request_body_codec: BodyCodecKind::Utf8,
            response_body_codec: BodyCodecKind::ShiftJis,
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let epoch = Uuid::new_v4();
    let resolver = Arc::new(WorkspaceBodyCodecResolver::new());
    resolver.install_listener(epoch, Uuid::new_v4(), &listener);
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        Arc::new(StaticRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::new(Vec::new())),
        }),
        Arc::new(InMemorySessionStore::default()),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(8)),
        test_capture_repository(),
    )
    .with_body_codec_resolver(resolver);
    let context = test_context(
        epoch,
        Uuid::new_v4(),
        ChannelId::new(listener.id.to_string()).unwrap(),
    );
    let message = request_message("body");
    open_test_connection(&pipeline, &context).await;
    let mut request = message;
    pipeline
        .apply_request_policy(&context, &mut request)
        .await
        .unwrap();

    let mut response = response_message();
    pipeline
        .apply_response_policy(&context, &mut response)
        .await
        .unwrap();

    release_tx.send(()).unwrap();
    occupied.await.unwrap().unwrap();
}
