#[tokio::test]
async fn request_handler_preserves_stable_fault_error_codes_on_connection_close() {
    let cases = [
        (
            vec![FaultAction::DisconnectBeforeUpstream],
            None,
            ErrorCode::ClientDisconnected.as_str(),
        ),
        (
            Vec::new(),
            Some(ErrorCode::UpstreamConnectTimeout),
            ErrorCode::UpstreamConnectTimeout.as_str(),
        ),
    ];

    for (request_actions, connector_error, expected_code) in cases {
        let ports = Arc::new(ClosedResultPorts {
            request_actions,
            ..ClosedResultPorts::default()
        });
        let upstream: Arc<dyn UpstreamConnector> = connector_error.map_or_else(
            || Arc::new(EchoConnector) as Arc<dyn UpstreamConnector>,
            |code| Arc::new(FailingConnector(code)) as Arc<dyn UpstreamConnector>,
        );
        let supervisor = ProxySupervisor::new(
            Arc::new(TokioListenerBinder),
            service_with_connector(ports.clone(), upstream),
        );
        let started = supervisor.start(config()).await.unwrap();
        let _response = exchange(started.listeners[&channel_id("beta")], b"request").await;
        tokio::time::timeout(Duration::from_secs(1), ports.closed.notified())
            .await
            .expect("connection closed callback");
        assert_eq!(
            ports.closed_code.lock().unwrap().as_deref(),
            Some(expected_code)
        );
        supervisor.stop().await.unwrap();
    }
}

#[tokio::test]
async fn two_ports_use_http11_close_and_preserve_body_bytes() {
    let ports = Arc::new(RecordingPorts::default());
    let supervisor = ProxySupervisor::new(Arc::new(TokioListenerBinder), service(ports.clone()));
    let started = supervisor.start(config()).await.unwrap();
    assert_eq!(started.state, ProxyState::Running);
    assert_eq!(started.listeners.len(), 2);

    let raw_body = [0x81, 0x00, 0xff];
    for address in started.listeners.values() {
        let response = exchange(*address, &raw_body).await;
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert!(
            response
                .windows(b"connection: close".len())
                .any(|window| window.eq_ignore_ascii_case(b"connection: close"))
        );
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        assert_eq!(&response[split + 4..], &raw_body);
    }

    {
        let captured = ports.bodies.lock().unwrap();
        assert_eq!(
            captured.as_slice(),
            &[
                Bytes::copy_from_slice(&raw_body),
                Bytes::copy_from_slice(&raw_body)
            ]
        );
    }
    {
        let connection_ids = ports.connection_ids.lock().unwrap();
        assert_eq!(connection_ids.len(), 2);
        assert_ne!(connection_ids[0], connection_ids[1]);
        assert!(connection_ids.iter().all(|id| !id.is_nil()));
    }
    assert_eq!(supervisor.stop().await.unwrap().state, ProxyState::Stopped);
}

#[tokio::test]
async fn one_http11_connection_processes_two_framed_requests_in_one_exchange() {
    let ports = Arc::new(RecordingPorts::default());
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service_with_connector(ports.clone(), Arc::new(SequencedRawConnector::default())),
    );
    let started = supervisor.start(config()).await.unwrap();
    let mut stream = TcpStream::connect(started.listeners[&channel_id("alpha")])
        .await
        .unwrap();

    for (sequence, body) in [b"first".as_slice(), b"second".as_slice()]
        .into_iter()
        .enumerate()
    {
        let request = format!(
            "POST /settle HTTP/1.1\r\nHost: app\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        stream.write_all(body).await.unwrap();
        let head = read_http_head(&mut stream).await;
        assert!(
            head.starts_with(format!("HTTP/1.1 200 Sequence {sequence}\r\n").as_bytes()),
            "head: {head:?}"
        );
        assert!(
            head.windows(format!("X-Sequence: exact-{sequence}\r\n").len())
                .any(|window| window == format!("X-Sequence: exact-{sequence}\r\n").as_bytes()),
            "every response must use its own encoded canonical head: {head:?}"
        );
        assert!(
            !head
                .windows(b"connection: close".len())
                .any(|window| window.eq_ignore_ascii_case(b"connection: close")),
            "long-lived Exchange must not force HTTP keep-alive off"
        );
        let mut response_body = vec![0; body.len()];
        stream.read_exact(&mut response_body).await.unwrap();
        assert_eq!(response_body, body);
    }

    drop(stream);
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if ports.bodies.lock().unwrap().len() == 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both messages must pass through one connection Exchange");
    assert_eq!(ports.connection_ids.lock().unwrap().len(), 1);
    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn keep_alive_request_forwards_chunked_close_response_without_header_conflict() {
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_server = tokio::spawn(serve_chunked_close_upstream(upstream_listener));
    let connector = HyperUpstreamConnector {
        address: upstream_address,
        host: "upstream.test".into(),
        host_header: "upstream.test".into(),
        rewrite_host: true,
        tls: None,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        limits: MessageLimits::default(),
    };
    let ports = Arc::new(ClosedResultPorts::default());
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service_with_connector(ports.clone(), Arc::new(connector)),
    );
    let started = supervisor.start(config()).await.unwrap();

    let response = exchange_raw(
        started.listeners[&channel_id("alpha")],
        b"GET /ex-tms/v1/terminal-status HTTP/1.1\r\nHost: app\r\nConnection: Keep-Alive\r\n\r\n",
    )
    .await;
    tokio::time::timeout(Duration::from_secs(1), ports.closed.notified())
        .await
        .expect("connection closed callback");

    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("complete HTTP response head");
    assert!(response.starts_with(b"HTTP/1.1 404 Not Found\r\n"));
    assert!(
        !response[..split]
            .windows(b"content-length".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-length"))
    );
    assert!(
        response[..split]
            .windows(b"transfer-encoding: chunked".len())
            .any(|window| window.eq_ignore_ascii_case(b"transfer-encoding: chunked"))
    );
    assert_eq!(
        &response[split + 4..],
        b"15\r\n{\"result_code\":\"T29\"}\r\n0\r\n\r\n"
    );
    assert_eq!(ports.closed_code.lock().unwrap().as_deref(), None);

    supervisor.stop().await.unwrap();
    upstream_server.await.unwrap();
}

#[tokio::test]
async fn conflicting_upstream_length_is_removed_when_transfer_encoding_is_present() {
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_server = tokio::spawn(serve_chunked_conflicting_length_upstream(upstream_listener));
    let connector = HyperUpstreamConnector {
        address: upstream_address,
        host: "upstream.test".into(),
        host_header: "upstream.test".into(),
        rewrite_host: true,
        tls: None,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        limits: MessageLimits::default(),
    };
    let ports = Arc::new(ClosedResultPorts::default());
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service_with_connector(ports.clone(), Arc::new(connector)),
    );
    let started = supervisor.start(config()).await.unwrap();

    let response = exchange_raw(
        started.listeners[&channel_id("alpha")],
        b"GET /conflicting-framing HTTP/1.1\r\nHost: app\r\nConnection: Keep-Alive\r\n\r\n",
    )
    .await;
    tokio::time::timeout(Duration::from_secs(1), ports.closed.notified())
        .await
        .expect("connection closed callback");

    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("complete HTTP response head");
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    assert!(
        !response[..split]
            .windows(b"content-length".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-length"))
    );
    assert!(
        response[..split]
            .windows(b"transfer-encoding: chunked".len())
            .any(|window| window.eq_ignore_ascii_case(b"transfer-encoding: chunked"))
    );
    assert_eq!(&response[split + 4..], b"2\r\n{}\r\n0\r\n\r\n");
    assert_eq!(ports.closed_code.lock().unwrap().as_deref(), None);

    supervisor.stop().await.unwrap();
    upstream_server.await.unwrap();
}

#[tokio::test]
async fn close_delimited_transfer_coding_is_reframed_as_chunked_downstream() {
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_server = tokio::spawn(serve_close_delimited_gzip_upstream(upstream_listener));
    let connector = HyperUpstreamConnector {
        address: upstream_address,
        host: "upstream.test".into(),
        host_header: "upstream.test".into(),
        rewrite_host: true,
        tls: None,
        connect_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        limits: MessageLimits::default(),
    };
    let ports = Arc::new(ClosedResultPorts::default());
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service_with_connector(ports.clone(), Arc::new(connector)),
    );
    let started = supervisor.start(config()).await.unwrap();

    let response = exchange_raw(
        started.listeners[&channel_id("alpha")],
        b"GET /gzip-transfer-coding HTTP/1.1\r\nHost: app\r\nConnection: Keep-Alive\r\n\r\n",
    )
    .await;
    tokio::time::timeout(Duration::from_secs(1), ports.closed.notified())
        .await
        .expect("connection closed callback");

    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("complete HTTP response head");
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    assert!(
        !response[..split]
            .windows(b"content-length".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-length"))
    );
    assert!(
        response[..split]
            .windows(b"transfer-encoding: gzip, chunked".len())
            .any(|window| window.eq_ignore_ascii_case(b"transfer-encoding: gzip, chunked"))
    );
    assert_eq!(&response[split + 4..], b"7\r\nencoded\r\n0\r\n\r\n");
    assert_eq!(ports.closed_code.lock().unwrap().as_deref(), None);

    supervisor.stop().await.unwrap();
    upstream_server.await.unwrap();
}

#[tokio::test]
async fn mock_response_writes_arbitrary_body_bytes_without_codec_round_trip() {
    let body = Bytes::from_static(&[0x00, 0x80, 0xff, b'{']);
    let ports = Arc::new(ClosedResultPorts {
        request_actions: vec![FaultAction::MockResponse {
            status: StatusCode::IM_A_TEAPOT,
            headers: HeaderMap::new(),
            body: body.clone(),
        }],
        ..ClosedResultPorts::default()
    });
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service_with_connector(ports, Arc::new(EchoConnector)),
    );
    let started = supervisor.start(config()).await.unwrap();

    let response = exchange(started.listeners[&channel_id("alpha")], b"ignored").await;
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("response header terminator");

    assert!(response.starts_with(b"HTTP/1.1 418 I'm a teapot\r\n"));
    assert_eq!(&response[split + 4..], body.as_ref());
    assert!(
        response[..split]
            .windows(b"content-length: 4".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-length: 4"))
    );

    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn runtime_stopping_is_delivered_before_active_connections_join() {
    let ports = Arc::new(LifecyclePorts::default());
    let supervisor = ProxySupervisor::new(Arc::new(TokioListenerBinder), service(ports.clone()));
    let started = supervisor.start(config()).await.unwrap();
    let _client = TcpStream::connect(started.listeners[&channel_id("alpha")])
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), ports.opened.notified())
        .await
        .expect("connection opened");

    supervisor.stop().await.unwrap();

    let events = ports.events.lock().unwrap();
    let stopping = events
        .iter()
        .position(|event| *event == "runtime_stopping")
        .unwrap();
    let closed = events
        .iter()
        .position(|event| *event == "connection_closed")
        .unwrap();
    assert!(stopping < closed, "events: {events:?}");
}

#[tokio::test]
async fn stop_cancels_twenty_active_clients() {
    let supervisor = Arc::new(ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service(Arc::new(NoopPipelinePorts)),
    ));
    let started = supervisor.start(config()).await.unwrap();
    let address = started.listeners[&channel_id("alpha")];
    let mut clients = Vec::new();
    for _ in 0..20 {
        clients.push(TcpStream::connect(address).await.unwrap());
    }
    assert_eq!(supervisor.stop().await.unwrap().state, ProxyState::Stopped);
    for mut client in clients {
        let mut byte = [0u8; 1];
        let result = tokio::time::timeout(Duration::from_secs(1), client.read(&mut byte))
            .await
            .expect("client connection is cancelled");
        match result {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ) => {}
            other => panic!("unexpected client result after stop: {other:?}"),
        }
    }
}
