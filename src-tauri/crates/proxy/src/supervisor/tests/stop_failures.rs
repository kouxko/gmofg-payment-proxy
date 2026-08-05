#[tokio::test]
async fn stopping_callback_failure_is_reported_and_keeps_faulted_state() {
    for failure in [StoppingFailure::Panic, StoppingFailure::Timeout] {
        let ports = Arc::new(FailingStoppingPorts(failure));
        let supervisor =
            ProxySupervisor::new(Arc::new(TokioListenerBinder), test_service(ports.clone()));
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
            ports,
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

        let error = supervisor
            .stop()
            .await
            .expect_err("failed cleanup callback must fail stop");
        assert_eq!(error.code, ErrorCode::Internal.as_str());
        let snapshot = supervisor.snapshot().await;
        assert_eq!(snapshot.state, ProxyState::Faulted);
        assert!(snapshot.runtime_epoch.is_none());
        assert!(
            snapshot
                .fault
                .as_deref()
                .is_some_and(|fault| fault.contains("runtime_stopping callback"))
        );
    }
}

async fn install_synthetic_runtime(
    supervisor: &ProxySupervisor,
    ports: Arc<dyn PipelinePorts>,
) -> Uuid {
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
        ports,
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
    epoch
}

#[tokio::test]
async fn failed_stopping_callback_is_retried_until_later_stop_succeeds() {
    for failure in [StoppingFailure::Panic, StoppingFailure::Timeout] {
        let ports = Arc::new(RetryingStoppingPorts {
            attempts: AtomicUsize::new(0),
            failures_before_success: 2,
            failure,
        });
        let supervisor =
            ProxySupervisor::new(Arc::new(TokioListenerBinder), test_service(ports.clone()));
        install_synthetic_runtime(&supervisor, ports.clone()).await;

        for expected_attempts in [1, 2] {
            supervisor
                .stop()
                .await
                .expect_err("pending cleanup must keep stop faulted until callback succeeds");
            assert_eq!(
                supervisor.snapshot().await.state,
                ProxyState::Faulted,
                "failed retry must not publish Stopped"
            );
            assert_eq!(
                ports.attempts.load(Ordering::Acquire),
                expected_attempts,
                "each stop retries the pending cleanup exactly once"
            );
        }

        let stopped = supervisor
            .stop()
            .await
            .expect("third cleanup callback attempt succeeds");
        assert_eq!(stopped.state, ProxyState::Stopped);
        assert_eq!(ports.attempts.load(Ordering::Acquire), 3);
    }
}

#[derive(Debug)]
struct CountingBinder {
    binds: AtomicUsize,
}

#[async_trait]
impl ListenerBinder for CountingBinder {
    async fn bind(&self, address: SocketAddr) -> io::Result<Arc<dyn BoundListener>> {
        self.binds.fetch_add(1, Ordering::AcqRel);
        TokioListenerBinder.bind(address).await
    }
}
