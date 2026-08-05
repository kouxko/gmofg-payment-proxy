#[tokio::test]
async fn starts_listens_and_stops_three_channels() {
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        test_service(Arc::new(NoopPipelinePorts)),
    );
    let mut config = test_config();
    config.channels.push(ChannelConfig {
        channel: channel_id("gamma"),
        enabled: true,
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        upstream_url: "http://gamma.test/".into(),
    });

    let started = supervisor
        .start(config)
        .await
        .expect("start three channels");
    assert_eq!(started.state, ProxyState::Running);
    assert_eq!(started.listeners.len(), 3);
    for expected in ["alpha", "beta", "gamma"] {
        let address = started
            .listeners
            .get(&channel_id(expected))
            .expect("listener address exists");
        assert_ne!(address.port(), 0);
        tokio::net::TcpStream::connect(address)
            .await
            .expect("configured listener accepts connections");
    }

    let stopped = supervisor.stop().await.expect("stop three channels");
    assert_eq!(stopped.state, ProxyState::Stopped);
    assert!(stopped.listeners.is_empty());
}

#[derive(Debug)]
struct BlockingFailFactory {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl RuntimeServiceFactory for BlockingFailFactory {
    async fn build(&self, _config: &ProxyConfig) -> Result<BTreeMap<ChannelId, ConnectionService>> {
        self.entered.notify_one();
        self.release.notified().await;
        Err(ProxyError::new(
            ErrorCode::CertificateNotReady,
            "injected startup failure",
        ))
    }
}

#[tokio::test]
async fn aborted_start_waiter_does_not_strand_starting_state() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let supervisor = Arc::new(ProxySupervisor::with_factory(
        Arc::new(TokioListenerBinder),
        Arc::new(BlockingFailFactory {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        }),
    ));
    let waiter = {
        let supervisor = Arc::clone(&supervisor);
        tokio::spawn(async move { supervisor.start(test_config()).await })
    };
    entered.notified().await;
    waiter.abort();
    release.notify_one();

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if supervisor.snapshot().await.state == ProxyState::Faulted {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("detached start operation reaches a stable terminal state");
}

#[derive(Debug)]
struct BlockingStoppingPorts {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[derive(Debug, Clone, Copy)]
enum StoppingFailure {
    Panic,
    Timeout,
}

#[derive(Debug)]
struct FailingStoppingPorts(StoppingFailure);

#[derive(Debug)]
struct RetryingStoppingPorts {
    attempts: AtomicUsize,
    failures_before_success: usize,
    failure: StoppingFailure,
}

impl HandshakePolicy for FailingStoppingPorts {}

#[async_trait]
impl PipelinePorts for FailingStoppingPorts {
    async fn runtime_stopping(&self, _epoch: Uuid) {
        match self.0 {
            StoppingFailure::Panic => panic!("injected runtime_stopping panic"),
            StoppingFailure::Timeout => pending::<()>().await,
        }
    }
}

impl HandshakePolicy for RetryingStoppingPorts {}

#[async_trait]
impl PipelinePorts for RetryingStoppingPorts {
    async fn runtime_stopping(&self, _epoch: Uuid) {
        let attempt = self.attempts.fetch_add(1, Ordering::AcqRel);
        if attempt < self.failures_before_success {
            match self.failure {
                StoppingFailure::Panic => panic!("injected retryable runtime_stopping panic"),
                StoppingFailure::Timeout => pending::<()>().await,
            }
        }
    }
}

impl HandshakePolicy for BlockingStoppingPorts {}

#[async_trait]
impl PipelinePorts for BlockingStoppingPorts {
    async fn runtime_stopping(&self, _epoch: Uuid) {
        self.entered.notify_one();
        self.release.notified().await;
    }
}

#[tokio::test]
async fn aborted_stop_waiter_does_not_strand_stopping_state() {
    let ports = Arc::new(BlockingStoppingPorts {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let supervisor = Arc::new(ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        test_service(ports.clone()),
    ));
    let epoch = Uuid::new_v4();
    let cancellation = CancellationToken::new();
    let listener_cancel = cancellation.clone();
    let watchdog_cancel = cancellation.clone();
    *supervisor.core.runtime.lock().await = Some(Runtime {
        epoch,
        cancellation: cancellation.clone(),
        listener_tasks: vec![tokio::spawn(async move {
            listener_cancel.cancelled().await;
        })],
        watchdog: tokio::spawn(async move {
            watchdog_cancel.cancelled().await;
        }),
        ports: ports.clone(),
        stopping_notified: Arc::new(StoppingNotification::default()),
    });
    *supervisor
        .core
        .active_cancellation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cancellation);
    {
        let mut lifecycle = supervisor.core.lifecycle.write().await;
        lifecycle.state = ProxyState::Running;
        lifecycle.epoch = Some(epoch);
    }

    let waiter = {
        let supervisor = Arc::clone(&supervisor);
        tokio::spawn(async move { supervisor.stop().await })
    };
    ports.entered.notified().await;
    waiter.abort();
    ports.release.notify_one();

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if supervisor.snapshot().await.state == ProxyState::Stopped {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("detached stop operation reaches Stopped");
}
