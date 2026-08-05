#[tokio::test]
async fn failed_stopping_callback_blocks_start_until_cleanup_retry_succeeds() {
    let ports = Arc::new(RetryingStoppingPorts {
        attempts: AtomicUsize::new(0),
        failures_before_success: 2,
        failure: StoppingFailure::Panic,
    });
    let binder = Arc::new(CountingBinder {
        binds: AtomicUsize::new(0),
    });
    let supervisor = ProxySupervisor::new(binder.clone(), test_service(ports.clone()));
    let old_epoch = install_synthetic_runtime(&supervisor, ports.clone()).await;

    supervisor
        .stop()
        .await
        .expect_err("initial cleanup callback fails");
    supervisor
        .start(test_config())
        .await
        .expect_err("start must retry and remain blocked by pending cleanup");
    let blocked = supervisor.snapshot().await;
    assert_eq!(blocked.state, ProxyState::Faulted);
    assert!(blocked.runtime_epoch.is_none());
    assert_eq!(
        binder.binds.load(Ordering::Acquire),
        0,
        "start cannot bind a new epoch before pending cleanup succeeds"
    );
    assert_eq!(ports.attempts.load(Ordering::Acquire), 2);

    let running = supervisor
        .start(test_config())
        .await
        .expect("start may proceed only after cleanup retry succeeds");
    assert_eq!(running.state, ProxyState::Running);
    assert_ne!(running.runtime_epoch, Some(old_epoch));
    assert_eq!(binder.binds.load(Ordering::Acquire), 2);
    assert_eq!(ports.attempts.load(Ordering::Acquire), 3);
    supervisor.stop().await.unwrap();
}

#[derive(Debug)]
struct PendingListener {
    address: SocketAddr,
    accept_dropped: Arc<AtomicBool>,
}

struct AcceptGuard(Arc<AtomicBool>);

impl Drop for AcceptGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[async_trait]
impl BoundListener for PendingListener {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.address)
    }

    async fn accept(&self) -> io::Result<(BoxIo, SocketAddr)> {
        let _guard = AcceptGuard(Arc::clone(&self.accept_dropped));
        pending().await
    }
}

#[derive(Debug)]
struct PanicListener(SocketAddr);

#[async_trait]
impl BoundListener for PanicListener {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.0)
    }

    async fn accept(&self) -> io::Result<(BoxIo, SocketAddr)> {
        panic!("injected listener panic");
    }
}

#[derive(Debug)]
struct PanicBinder {
    binds: AtomicUsize,
    sibling_accept_dropped: Arc<AtomicBool>,
}

#[async_trait]
impl ListenerBinder for PanicBinder {
    async fn bind(&self, _address: SocketAddr) -> io::Result<Arc<dyn BoundListener>> {
        let index = self.binds.fetch_add(1, Ordering::Relaxed);
        if index == 0 {
            Ok(Arc::new(PendingListener {
                address: "127.0.0.1:31001".parse().unwrap(),
                accept_dropped: Arc::clone(&self.sibling_accept_dropped),
            }))
        } else {
            Ok(Arc::new(PanicListener("127.0.0.1:31002".parse().unwrap())))
        }
    }
}
