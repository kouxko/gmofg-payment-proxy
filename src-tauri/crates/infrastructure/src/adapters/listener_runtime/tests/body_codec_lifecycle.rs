use bytes::Bytes;
use intercept_proxy_domain::{BodyCodecKind, MessageStage};
use intercept_proxy_runtime::{
    ChannelId as RuntimeChannelId, ConnectionContext as RuntimeConnectionContext,
    Message as RuntimeMessage,
};

use crate::adapters::{WorkspaceBodyCodecResolver, pipeline::RuntimeBodyCodecResolver};

fn poll_once<F: std::future::Future>(future: std::pin::Pin<&mut F>) -> std::task::Poll<F::Output> {
    future.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

#[derive(Debug, Default)]
struct StoppingEpochs {
    started: std::sync::Mutex<Vec<Uuid>>,
    epochs: std::sync::Mutex<Vec<Uuid>>,
    notified: tokio::sync::Notify,
}

impl HandshakePolicy for StoppingEpochs {}

#[async_trait::async_trait]
impl PipelinePorts for StoppingEpochs {
    async fn runtime_started(&self, epoch: Uuid) {
        self.started.lock().unwrap().push(epoch);
    }

    async fn runtime_stopping(&self, epoch: Uuid) {
        self.epochs.lock().unwrap().push(epoch);
        self.notified.notify_one();
    }
}

impl StoppingEpochs {
    fn snapshot(&self) -> Vec<Uuid> {
        self.epochs.lock().unwrap().clone()
    }

    fn started_snapshot(&self) -> Vec<Uuid> {
        self.started.lock().unwrap().clone()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn listener_start_activates_exact_epoch_before_running_and_stop_retires_it() {
    let listener = available_http_listener("epoch lifecycle").await;
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let (runtime, _resolver, pipeline) = runtime_with_recording_pipeline();

    runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();
    let epoch = runtime.running.lock().await[&listener.id].runtime_epoch;
    assert_eq!(pipeline.started_snapshot(), vec![epoch]);
    assert!(pipeline.snapshot().is_empty());

    runtime.stop(listener.id).await.unwrap();
    assert_eq!(pipeline.snapshot(), vec![epoch]);
}

#[tokio::test]
async fn body_codec_snapshot_is_installed_for_exact_epoch_and_removed_on_stop() {
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind_address = reservation.local_addr().unwrap();
    drop(reservation);
    let listener = ProxyListener {
        id: ListenerId::new(),
        name: "codec lifecycle".into(),
        enabled: false,
        bind_address: bind_address.ip().to_string(),
        port: bind_address.port(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            request_body_codec: BodyCodecKind::Raw,
            response_body_codec: BodyCodecKind::ShiftJis,
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let resolver = Arc::new(WorkspaceBodyCodecResolver::new());
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    runtime.set_body_codec_resolver(Arc::clone(&resolver));
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));

    runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();
    let runtime_epoch = runtime.runtime_epochs.read()[&workspace.id];
    let context = RuntimeConnectionContext {
        runtime_epoch,
        connection_id: Uuid::new_v4(),
        channel: RuntimeChannelId::new(listener.id.to_string()).unwrap(),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
        accepted_at: std::time::SystemTime::now(),
        tls_peer: None,
    };
    let message = RuntimeMessage::from_raw_http1_head(
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
        Bytes::new(),
    )
    .unwrap();
    assert_eq!(
        resolver
            .resolve(&context, MessageStage::Response, &message)
            .unwrap()
            .expect("frozen response codec")
            .id(),
        "shift-jis"
    );

    runtime.stop(listener.id).await.unwrap();
    assert!(
        resolver
            .resolve(&context, MessageStage::Response, &message)
            .is_err(),
        "stopped runtime epoch retained a codec snapshot"
    );

    runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();
    let restarted_epoch = runtime.runtime_epochs.read()[&workspace.id];
    assert_ne!(restarted_epoch, runtime_epoch);
    let restarted_context = RuntimeConnectionContext {
        runtime_epoch: restarted_epoch,
        ..context.clone()
    };
    assert_eq!(
        resolver
            .resolve(&restarted_context, MessageStage::Response, &message)
            .unwrap()
            .expect("restarted epoch codec")
            .id(),
        "shift-jis"
    );
    assert!(
        resolver
            .resolve(&context, MessageStage::Response, &message)
            .is_err()
    );
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn failed_listener_start_never_installs_a_body_codec_snapshot() {
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind_address = reservation.local_addr().unwrap();
    let listener = ProxyListener {
        id: ListenerId::new(),
        bind_address: bind_address.ip().to_string(),
        port: bind_address.port(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings::default()),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let resolver = Arc::new(WorkspaceBodyCodecResolver::new());
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    runtime.set_body_codec_resolver(Arc::clone(&resolver));
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));

    runtime
        .start(workspace.clone(), listener.clone())
        .await
        .expect_err("occupied address must fail start");
    assert!(!runtime.runtime_epochs.read().contains_key(&workspace.id));
    let context = RuntimeConnectionContext {
        runtime_epoch: Uuid::new_v4(),
        connection_id: Uuid::new_v4(),
        channel: RuntimeChannelId::new(listener.id.to_string()).unwrap(),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
        accepted_at: std::time::SystemTime::now(),
        tls_peer: None,
    };
    let message = RuntimeMessage::from_raw_http1_head(
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
        Bytes::new(),
    )
    .unwrap();
    assert!(
        resolver
            .resolve(&context, MessageStage::Response, &message)
            .is_err(),
        "failed start leaked a codec snapshot"
    );
}

async fn available_http_listener(name: &str) -> ProxyListener {
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind_address = reservation.local_addr().unwrap();
    drop(reservation);
    ProxyListener {
        id: ListenerId::new(),
        name: name.into(),
        bind_address: bind_address.ip().to_string(),
        port: bind_address.port(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            response_body_codec: BodyCodecKind::ShiftJis,
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    }
}

fn response_context(epoch: Uuid, listener_id: ListenerId) -> RuntimeConnectionContext {
    RuntimeConnectionContext {
        runtime_epoch: epoch,
        connection_id: Uuid::new_v4(),
        channel: RuntimeChannelId::new(listener_id.to_string()).unwrap(),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
        accepted_at: std::time::SystemTime::now(),
        tls_peer: None,
    }
}

fn empty_response() -> RuntimeMessage {
    RuntimeMessage::from_raw_http1_head(
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
        Bytes::new(),
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn overlapping_stop_and_restart_cannot_remove_the_new_epoch_snapshot() {
    let listener = available_http_listener("overlap").await;
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let resolver = Arc::new(WorkspaceBodyCodecResolver::new());
    let runtime = Arc::new(test_listener_runtime(Arc::new(
        SqliteStore::in_memory().unwrap(),
    )));
    runtime.set_body_codec_resolver(Arc::clone(&resolver));
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
    runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();
    let old_epoch = runtime.runtime_epochs.read()[&workspace.id];
    let (reached, release, _completed) = runtime.install_stop_barrier(listener.id).await;
    let reached_wait = reached.notified();
    let listener_id = listener.id;
    let stopping = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        async move { runtime.stop(listener_id).await }
    });
    reached_wait.await;

    let mut restarting = Box::pin(runtime.start(workspace.clone(), listener.clone()));
    assert!(poll_once(restarting.as_mut()).is_pending());
    release.notify_one();
    stopping.await.unwrap().unwrap();
    restarting.await.unwrap();
    let new_epoch = runtime.runtime_epochs.read()[&workspace.id];
    assert_ne!(new_epoch, old_epoch);

    assert_eq!(runtime.runtime_epochs.read()[&workspace.id], new_epoch);
    assert_eq!(
        resolver
            .resolve(
                &response_context(new_epoch, listener.id),
                MessageStage::Response,
                &empty_response(),
            )
            .unwrap()
            .expect("new snapshot survives old stop")
            .id(),
        "shift-jis"
    );
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn old_stop_cannot_remove_same_epoch_restart_snapshot_while_sibling_runs() {
    let first = available_http_listener("same epoch restart").await;
    let mut workspace = ProxyWorkspace {
        listeners: vec![first.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let resolver = Arc::new(WorkspaceBodyCodecResolver::new());
    let runtime = Arc::new(test_listener_runtime(Arc::new(
        SqliteStore::in_memory().unwrap(),
    )));
    runtime.set_body_codec_resolver(Arc::clone(&resolver));
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
    runtime
        .start(workspace.clone(), first.clone())
        .await
        .unwrap();
    let sibling = available_http_listener("sibling").await;
    workspace.listeners.push(sibling.clone());
    workspace.validate().unwrap();
    runtime
        .start(workspace.clone(), sibling.clone())
        .await
        .unwrap();
    let shared_epoch = runtime.runtime_epochs.read()[&workspace.id];
    let (reached, release, _completed) = runtime.install_stop_barrier(first.id).await;
    let reached_wait = reached.notified();
    let first_id = first.id;
    let stopping = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        async move { runtime.stop(first_id).await }
    });
    reached_wait.await;

    let mut restarting = Box::pin(runtime.start(workspace.clone(), first.clone()));
    assert!(poll_once(restarting.as_mut()).is_pending());
    release.notify_one();
    stopping.await.unwrap().unwrap();
    restarting.await.unwrap();
    assert_eq!(runtime.runtime_epochs.read()[&workspace.id], shared_epoch);

    assert_eq!(
        resolver
            .resolve(
                &response_context(shared_epoch, first.id),
                MessageStage::Response,
                &empty_response(),
            )
            .unwrap()
            .expect("new same-epoch run owns the snapshot")
            .id(),
        "shift-jis"
    );
    runtime.stop(first.id).await.unwrap();
    runtime.stop(sibling.id).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn stop_if_run_token_waits_for_the_same_listener_stop_gate() {
    let listener = available_http_listener("run token gate").await;
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let runtime = Arc::new(test_listener_runtime(Arc::new(
        SqliteStore::in_memory().unwrap(),
    )));
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
    runtime.start(workspace, listener.clone()).await.unwrap();
    let run_token = crate::adapters::external_package_server::ExternalPackageListenerRuntime::current_run_token(
        runtime.as_ref(),
        listener.id,
    )
    .await
    .unwrap();
    let (reached, release, _completed) = runtime.install_stop_barrier(listener.id).await;
    let reached_wait = reached.notified();
    let stopping = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let listener_id = listener.id;
        async move { runtime.stop(listener_id).await }
    });
    reached_wait.await;

    let mut token_stop = Box::pin(
        crate::adapters::external_package_server::ExternalPackageListenerRuntime::stop_if_run_token(
            runtime.as_ref(),
            listener.id,
            run_token,
        ),
    );
    assert!(poll_once(token_stop.as_mut()).is_pending());
    release.notify_one();
    stopping.await.unwrap().unwrap();
    assert!(token_stop.await.unwrap().is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn pending_start_keeps_the_workspace_epoch_owned_while_last_listener_stops() {
    let first = available_http_listener("first").await;
    let mut workspace = ProxyWorkspace {
        listeners: vec![first.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let resolver = Arc::new(WorkspaceBodyCodecResolver::new());
    let runtime = Arc::new(test_listener_runtime(Arc::new(
        SqliteStore::in_memory().unwrap(),
    )));
    runtime.set_body_codec_resolver(Arc::clone(&resolver));
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
    runtime
        .start(workspace.clone(), first.clone())
        .await
        .unwrap();
    let shared_epoch = runtime.runtime_epochs.read()[&workspace.id];
    let second = available_http_listener("pending").await;
    workspace.listeners.push(second.clone());
    workspace.validate().unwrap();
    let (reached, release, _completed) = runtime.install_start_barrier(second.id).await;
    let reached_wait = reached.notified();
    let starting = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let workspace = workspace.clone();
        let second = second.clone();
        async move { runtime.start(workspace, second).await }
    });
    reached_wait.await;

    runtime.stop(first.id).await.unwrap();
    assert_eq!(runtime.runtime_epochs.read()[&workspace.id], shared_epoch);
    release.notify_one();
    starting.await.unwrap().unwrap();

    assert_eq!(runtime.runtime_epochs.read()[&workspace.id], shared_epoch);
    assert_eq!(
        resolver
            .resolve(
                &response_context(shared_epoch, second.id),
                MessageStage::Response,
                &empty_response(),
            )
            .unwrap()
            .expect("pending listener snapshot")
            .id(),
        "shift-jis"
    );
    runtime.stop(second.id).await.unwrap();
}

fn runtime_with_recording_pipeline() -> (
    Arc<ListenerRuntimeAdapter>,
    Arc<WorkspaceBodyCodecResolver>,
    Arc<StoppingEpochs>,
) {
    let resolver = Arc::new(WorkspaceBodyCodecResolver::new());
    let pipeline = Arc::new(StoppingEpochs::default());
    let runtime = Arc::new(test_listener_runtime(Arc::new(
        SqliteStore::in_memory().unwrap(),
    )));
    runtime.set_body_codec_resolver(Arc::clone(&resolver));
    runtime.set_pipeline_ports(Arc::clone(&pipeline));
    (runtime, resolver, pipeline)
}
