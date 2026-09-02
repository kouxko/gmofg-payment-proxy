#[tokio::test]
async fn listener_panic_faults_epoch_and_cancels_sibling() {
    let sibling_accept_dropped = Arc::new(AtomicBool::new(false));
    let supervisor = ProxySupervisor::new(
        Arc::new(PanicBinder {
            binds: AtomicUsize::new(0),
            sibling_accept_dropped: Arc::clone(&sibling_accept_dropped),
        }),
        test_service(Arc::new(NoopPipelinePorts)),
    );
    assert_eq!(
        supervisor.start(test_config()).await.unwrap().state,
        ProxyState::Running
    );

    let faulted = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let snapshot = supervisor.snapshot().await;
            if snapshot.state == ProxyState::Faulted {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("watchdog observes listener panic");
    assert!(
        faulted
            .fault
            .as_deref()
            .is_some_and(|fault| fault.contains("listener panicked"))
    );
    assert!(
        sibling_accept_dropped.load(Ordering::Acquire),
        "fault cancellation drops the sibling accept future"
    );
    supervisor.stop().await.unwrap();
}

struct TaskDropGuard(Arc<AtomicBool>);

impl Drop for TaskDropGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[tokio::test]
async fn shutdown_aborts_tasks_that_ignore_cancellation_after_grace_period() {
    let listener_dropped = Arc::new(AtomicBool::new(false));
    let watchdog_dropped = Arc::new(AtomicBool::new(false));
    let listener_started = Arc::new(tokio::sync::Notify::new());
    let watchdog_started = Arc::new(tokio::sync::Notify::new());
    let listener_task = {
        let dropped = Arc::clone(&listener_dropped);
        let started = Arc::clone(&listener_started);
        tokio::spawn(async move {
            let _guard = TaskDropGuard(dropped);
            started.notify_one();
            pending::<()>().await;
        })
    };
    let watchdog = {
        let dropped = Arc::clone(&watchdog_dropped);
        let started = Arc::clone(&watchdog_started);
        tokio::spawn(async move {
            let _guard = TaskDropGuard(dropped);
            started.notify_one();
            pending::<()>().await;
        })
    };
    listener_started.notified().await;
    watchdog_started.notified().await;

    let cancellation = CancellationToken::new();
    let error = shutdown_runtime(Runtime {
        epoch: Uuid::new_v4(),
        cancellation,
        listener_tasks: vec![listener_task],
        watchdog,
        ports: Arc::new(NoopPipelinePorts),
        stopping_notified: Arc::new(StoppingNotification::default()),
    })
    .await
    .expect_err("forced abort is a shutdown failure");

    assert_eq!(error.code, ErrorCode::Internal.as_str());
    assert!(listener_dropped.load(Ordering::Acquire));
    assert!(watchdog_dropped.load(Ordering::Acquire));
}
