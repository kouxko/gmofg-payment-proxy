#[tokio::test]
async fn informational_continue_is_forwarded_before_the_canonical_final_response() {
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service(Arc::new(NoopPipelinePorts)),
    );
    let started = supervisor.start(config()).await.unwrap();
    let mut stream = TcpStream::connect(started.listeners[&channel_id("alpha")])
        .await
        .unwrap();
    stream
        .write_all(
            b"POST /continue HTTP/1.1\r\n\
Host: app\r\n\
Expect: 100-continue\r\n\
Content-Length: 4\r\n\r\n",
        )
        .await
        .unwrap();

    let informational = tokio::time::timeout(Duration::from_secs(1), read_http_head(&mut stream))
        .await
        .expect("100 Continue is not blocked by final-head preservation");
    assert!(informational.starts_with(b"HTTP/1.1 100 Continue\r\n"));

    stream.write_all(b"body").await.unwrap();
    let mut final_response = Vec::new();
    stream.read_to_end(&mut final_response).await.unwrap();
    assert!(final_response.starts_with(b"HTTP/1.1 200 OK\r\n"));
    assert!(final_response.ends_with(b"\r\n\r\nbody"));

    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn request_pipeline_captures_exact_binary_header_bytes_case_and_interleaving() {
    let ports = Arc::new(RecordingPorts::default());
    let supervisor = ProxySupervisor::new(Arc::new(TokioListenerBinder), service(ports.clone()));
    let started = supervisor.start(config()).await.unwrap();
    let request = b"POST /raw HTTP/1.1\r\n\
Host: app\r\n\
X-Trace:\t  first\x80 \t\r\n\
x-Other: middle\xff\r\n\
x-TRACE: second\r\n\
x-Other: last\r\n\
Content-Length: 0\r\n\r\n";

    let response = exchange_raw(started.listeners[&channel_id("alpha")], request).await;
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));

    {
        let captured = ports.messages.lock().unwrap();
        let message = captured.first().expect("captured request");
        assert_eq!(message.reconstruct(), Bytes::from_static(request));
        let observed = message
            .headers
            .iter()
            .filter(|header| {
                header.name.eq_ignore_ascii_case(b"x-trace")
                    || header.name.eq_ignore_ascii_case(b"x-other")
            })
            .map(|header| (header.name.as_ref(), header.value.as_ref()))
            .collect::<Vec<_>>();
        assert_eq!(
            observed,
            vec![
                (b"X-Trace".as_slice(), b"first\x80".as_slice()),
                (b"x-Other".as_slice(), b"middle\xff".as_slice()),
                (b"x-TRACE".as_slice(), b"second".as_slice()),
                (b"x-Other".as_slice(), b"last".as_slice()),
            ]
        );
    }

    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn downstream_wire_preserves_nonstandard_reason_and_exact_header_sequence() {
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service_with_connector(Arc::new(NoopPipelinePorts), Arc::new(RawResponseConnector)),
    );
    let started = supervisor.start(config()).await.unwrap();
    let response = exchange_raw(
        started.listeners[&channel_id("alpha")],
        b"GET / HTTP/1.1\r\nHost: app\r\nContent-Length: 0\r\n\r\n",
    )
    .await;

    assert!(response.starts_with(
        b"HTTP/1.1 299 Vendor Specific Result\r\n\
X-Trace: first\x80\r\n\
x-Other: middle\xff\r\n\
x-TRACE: second\r\n\
x-Other: last\r\n\
Content-Length: 2\r\n\
Connection: close\r\n\r\nok"
    ));

    supervisor.stop().await.unwrap();
}

#[tokio::test]
async fn upstream_informational_heads_are_forwarded_before_the_exact_final_response() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let upstream_address = listener.local_addr().unwrap();
    let (allow_final, wait_for_client) = oneshot::channel();
    let upstream = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 1024];
        let _ = stream.read(&mut request).await.unwrap();
        stream
            .write_all(b"HTTP/1.1 103 Early Hints\r\nLink:\t </style.css> \r\n\r\n")
            .await
            .unwrap();
        stream.flush().await.unwrap();
        wait_for_client
            .await
            .expect("client confirms the early response before final response");
        stream
            .write_all(
                b"HTTP/1.1 207 Product Final\r\nX-Final:\t yes \t\r\n\
Content-Length: 2\r\nConnection: close\r\n\r\nOK",
            )
            .await
            .unwrap();
    });
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
    let supervisor = ProxySupervisor::new(
        Arc::new(TokioListenerBinder),
        service_with_connector(Arc::new(NoopPipelinePorts), Arc::new(connector)),
    );
    let started = supervisor.start(config()).await.unwrap();
    let mut client = TcpStream::connect(started.listeners[&channel_id("alpha")])
        .await
        .unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: app\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let mut early = Vec::new();
    tokio::time::timeout(Duration::from_millis(500), async {
        let mut byte = [0u8; 1];
        while !early.ends_with(b"\r\n\r\n") {
            client.read_exact(&mut byte).await.unwrap();
            early.push(byte[0]);
        }
    })
    .await
    .expect("103 must reach the client while the upstream final response is blocked");
    assert_eq!(
        early,
        b"HTTP/1.1 103 Early Hints\r\nLink:\t </style.css> \r\n\r\n"
    );
    allow_final
        .send(())
        .expect("release the upstream final response");
    let mut final_response = Vec::new();
    client.read_to_end(&mut final_response).await.unwrap();
    assert!(final_response.starts_with(
        b"HTTP/1.1 207 Product Final\r\nX-Final:\t yes \t\r\n\
Content-Length: 2\r\nConnection: close\r\n\r\nOK"
    ));

    supervisor.stop().await.unwrap();
    upstream.await.unwrap();
}
