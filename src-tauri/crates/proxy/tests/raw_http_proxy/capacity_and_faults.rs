#[tokio::test]
async fn connection_capacity_rejects_excess_and_releases_permit_after_close() {
    let ports = Arc::new(LifecyclePorts::default());
    let supervisor = ProxySupervisor::new(Arc::new(TokioListenerBinder), service(ports.clone()));
    let mut limited_config = config();
    limited_config.max_connections = 1;
    let started = supervisor.start(limited_config).await.unwrap();
    let address = started.listeners[&channel_id("alpha")];

    let first = TcpStream::connect(address).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), ports.opened.notified())
        .await
        .expect("first connection acquired the only permit");

    let mut excess = TcpStream::connect(address).await.unwrap();
    let mut byte = [0_u8; 1];
    let rejected = tokio::time::timeout(Duration::from_secs(1), excess.read(&mut byte))
        .await
        .expect("excess connection is rejected promptly");
    match rejected {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
            ) => {}
        other => panic!("unexpected excess connection result: {other:?}"),
    }

    drop(first);
    tokio::time::timeout(Duration::from_secs(1), ports.closed.notified())
        .await
        .expect("closing the admitted connection releases its permit");

    let response = exchange(address, b"after-release").await;
    assert!(response.ends_with(b"after-release"));
    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn second_listener_bind_failure_rolls_back_first_listener() {
    let first_reservation = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let first = first_reservation.local_addr().unwrap();
    drop(first_reservation);
    let occupied = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let second = occupied.local_addr().unwrap();
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service(Arc::new(NoopPipelinePorts)),
    );
    let config = ProxyConfig {
        channels: vec![
            ChannelConfig {
                channel: channel_id("alpha"),
                enabled: true,
                listen_addr: first,
                upstream_url: "http://alpha.test/".into(),
            },
            ChannelConfig {
                channel: channel_id("beta"),
                enabled: true,
                listen_addr: second,
                upstream_url: "http://beta.test/".into(),
            },
        ],
        limits: MessageLimits::default(),
        max_connections: 500,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(2),
        rewrite_host: true,
        leaf_sans: vec!["localhost".into()],
    };

    let error = supervisor.start(config).await.unwrap_err();
    assert_eq!(error.code, "PORT_IN_USE");
    assert_eq!(supervisor.snapshot().await.state, ProxyState::Faulted);
    let rebound = tokio::net::TcpListener::bind(first)
        .await
        .expect("first transactional bind was rolled back");
    drop(rebound);
    drop(occupied);
}

#[tokio::test]
async fn truncation_sends_only_prefix_then_closes() {
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service(Arc::new(ResponseFaultPorts(vec![
            FaultAction::TruncateResponse(3),
        ]))),
    );
    let started = supervisor.start(config()).await.unwrap();
    let response = exchange(started.listeners[&channel_id("alpha")], b"abcdef").await;
    assert!(!response.is_empty(), "proxy returned no response bytes");
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    assert_eq!(&response[split + 4..], b"abc");
    assert!(
        response[..split]
            .windows(b"content-length: 6".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-length: 6"))
    );
    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn short_declared_length_still_writes_full_wire_body() {
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service(Arc::new(ResponseFaultPorts(vec![
            FaultAction::ContentLengthOffset(-3),
        ]))),
    );
    let started = supervisor.start(config()).await.unwrap();
    let response = exchange(started.listeners[&channel_id("alpha")], b"abcdef").await;
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    assert_eq!(&response[split + 4..], b"abcdef");
    assert!(
        response[..split]
            .windows(b"content-length: 3".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-length: 3"))
    );
    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn runtime_factory_receives_one_complete_snapshot_per_epoch() {
    let factory = Arc::new(RecordingFactory {
        calls: AtomicUsize::new(0),
        snapshots: Mutex::new(Vec::new()),
    });
    let supervisor = ProxySupervisor::with_factory(Arc::new(TokioListenerBinder), factory.clone());
    let expected = config();
    let first_epoch = supervisor
        .start(expected.clone())
        .await
        .unwrap()
        .runtime_epoch
        .unwrap();
    assert_eq!(factory.calls.load(Ordering::Relaxed), 1);
    {
        let snapshots = factory.snapshots.lock().unwrap();
        assert_eq!(
            snapshots[0].channels[0].upstream_url,
            expected.channels[0].upstream_url
        );
        assert_eq!(snapshots[0].connect_timeout, expected.connect_timeout);
        assert_eq!(snapshots[0].write_timeout, expected.write_timeout);
        assert_eq!(snapshots[0].read_timeout, expected.read_timeout);
        assert_eq!(snapshots[0].rewrite_host, expected.rewrite_host);
        assert_eq!(snapshots[0].leaf_sans, expected.leaf_sans);
    }
    supervisor.stop().await.unwrap();
    let second_epoch = supervisor
        .start(expected)
        .await
        .unwrap()
        .runtime_epoch
        .unwrap();
    assert_ne!(first_epoch, second_epoch);
    assert_eq!(factory.calls.load(Ordering::Relaxed), 2);
    supervisor.stop().await.unwrap();
}
