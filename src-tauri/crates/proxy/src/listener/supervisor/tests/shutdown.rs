use super::*;

#[tokio::test]
async fn cancellation_allows_primary_cleanup_within_grace() {
    let entered = Arc::new(Notify::new());
    let cleaned = Arc::new(Notify::new());
    let observer = Arc::new(RecordingObserver::default());
    let supervisor = Arc::new(supervisor(
        Arc::new(FakeHandler(HandlerMode::CleanupOnCancellation {
            entered: Arc::clone(&entered),
            cleaned: Arc::clone(&cleaned),
        })),
        Arc::clone(&observer),
        1,
        Duration::from_millis(50),
    ));
    let (listener, sender) = fake_listener();
    let cancellation = CancellationToken::new();
    let task = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        let cancellation = cancellation.clone();
        async move { supervisor.run_bound(listener, cancellation).await }
    });
    send_connection(&sender, "10.1.1.1:1001".parse().unwrap());
    entered.notified().await;
    cancellation.cancel();
    tokio::time::timeout(Duration::from_secs(1), cleaned.notified())
        .await
        .expect("primary cleanup completes before listener joins connections");
    assert!(matches!(
        task.await.unwrap().unwrap(),
        ListenerRunOutcome::Stopped { .. }
    ));
    assert_eq!(
        lock(&observer.terminal)[0].1,
        TerminalConnectionOutcome::Cancelled
    );
}

#[tokio::test]
async fn primary_exceeding_shutdown_grace_faults_listener_once() {
    let observer = Arc::new(RecordingObserver::default());
    let supervisor = Arc::new(supervisor(
        Arc::new(FakeHandler(HandlerMode::PendingPrimary)),
        Arc::clone(&observer),
        1,
        Duration::from_millis(20),
    ));
    let (listener, sender) = fake_listener();
    let cancellation = CancellationToken::new();
    let task = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        let cancellation = cancellation.clone();
        async move { supervisor.run_bound(listener, cancellation).await }
    });
    send_connection(&sender, "10.1.1.1:1001".parse().unwrap());
    observer
        .wait_until(|| lock(&observer.admitted).len() == 1)
        .await;
    cancellation.cancel();
    assert!(matches!(
        task.await.unwrap().unwrap(),
        ListenerRunOutcome::Faulted { code, .. }
            if code == LISTENER_SHUTDOWN_GRACE_EXCEEDED
    ));
    let terminal = lock(&observer.terminal);
    assert_eq!(terminal.len(), 1);
    assert_eq!(
        terminal[0].1,
        TerminalConnectionOutcome::ShutdownGraceExceeded
    );
}

#[tokio::test]
async fn cancellation_wins_when_primary_is_also_ready() {
    let observer = Arc::new(RecordingObserver::default());
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let (io, peer) = duplex(64);
    drop(peer);
    let context = ListenerRunContext::new(
        uuid::Uuid::new_v4(),
        ChannelId::new("cancel-first").unwrap(),
        Arc::new(SystemClock),
    )
    .connection("127.0.0.1:1000".parse().unwrap());
    let outcome = run_connection(
        Arc::new(FakeHandler(HandlerMode::Immediate)),
        observer,
        Box::new(io),
        context,
        cancellation,
        Duration::from_millis(20),
    )
    .await;
    assert_eq!(outcome, TerminalConnectionOutcome::Cancelled);
}

#[tokio::test]
async fn stop_joins_and_releases_the_bound_port_for_rebind() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let observer = Arc::new(RecordingObserver::default());
    let mut supervisor = supervisor(
        Arc::new(FakeHandler(HandlerMode::Immediate)),
        observer,
        1,
        Duration::from_millis(50),
    );
    supervisor.config.bind_addr = address;
    let supervisor = Arc::new(supervisor);
    let cancellation = CancellationToken::new();
    let task = tokio::spawn({
        let supervisor = Arc::clone(&supervisor);
        let cancellation = cancellation.clone();
        async move {
            supervisor
                .run_bound(Arc::new(TokioBoundListener(listener)), cancellation)
                .await
        }
    });
    cancellation.cancel();
    assert!(matches!(
        task.await.unwrap().unwrap(),
        ListenerRunOutcome::Stopped { local_addr } if local_addr == address
    ));
    let rebound = std::net::TcpListener::bind(address).unwrap();
    drop(rebound);
}
