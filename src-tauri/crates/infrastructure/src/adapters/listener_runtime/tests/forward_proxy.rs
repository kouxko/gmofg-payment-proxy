#[tokio::test]
async fn forward_absolute_form_http_enters_shared_pipeline() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 256];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
        }
        assert!(request.starts_with(b"GET /through-pipeline HTTP/1.1"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .await
            .unwrap();
    });
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind_address = reservation.local_addr().unwrap();
    drop(reservation);
    let listener = ProxyListener {
        id: ListenerId::new(),
        name: "forward".into(),
        enabled: false,
        bind_address: bind_address.ip().to_string(),
        port: bind_address.port(),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store
        .insert_workspace(&WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value: serde_json::to_value(&workspace).unwrap(),
            updated_at: Utc::now(),
        })
        .unwrap();
    let pipeline = Arc::new(CountingPipeline::default());
    let runtime = ListenerRuntimeAdapter::new(store);
    runtime.set_pipeline_ports(pipeline.clone());
    runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();

    let mut client = TcpStream::connect(bind_address).await.unwrap();
    client
            .write_all(
                format!(
                    "GET http://{upstream_address}/through-pipeline HTTP/1.1\r\nHost: {upstream_address}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut buffer = [0_u8; 256];
        while !response.ends_with(b"\r\n\r\nok") {
            let read = client.read(&mut buffer).await.unwrap();
            assert!(read > 0, "response ended before its complete body");
            response.extend_from_slice(&buffer[..read]);
        }
    })
    .await
    .expect("forward response timeout");
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert_eq!(pipeline.requests.load(Ordering::SeqCst), 1);
    assert_eq!(pipeline.responses.load(Ordering::SeqCst), 1);

    upstream_task.await.unwrap();
    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn fixed_server_listener_uses_selected_workspace_pipeline_and_preserves_body_bytes() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 256];
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "HTTP request ended before its complete body");
            request.extend_from_slice(&buffer[..read]);
            if request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .is_some_and(|head_end| request.len() >= head_end + 4 + 4)
            {
                break;
            }
        }
        assert!(request.ends_with(b"\x00\x81\xff\x7f"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\xff\x00ok")
            .await
            .unwrap();
    });

    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind_address = reservation.local_addr().unwrap();
    drop(reservation);
    let listener = ProxyListener {
        id: ListenerId::new(),
        name: "fixed server".into(),
        enabled: false,
        bind_address: bind_address.ip().to_string(),
        port: bind_address.port(),
        fixed_server: Some(FixedServerSettings {
            upstream_url: format!("http://{upstream_address}"),
            upstream_tls: UpstreamTlsSettings::default(),
        }),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        id: intercept_proxy_domain::WorkspaceId::new(),
        name: "test".into(),
        revision: Revision::INITIAL,
        listeners: vec![listener.clone()],
        metadata_extractors: Vec::new(),
        response_assertions: Vec::new(),
        rules: Vec::new(),
        fault_presets: Vec::new(),
        certificate_references: Vec::new(),
        android_network_profiles: Vec::new(),
    };
    workspace.validate().unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store
        .insert_workspace(&WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value: serde_json::to_value(&workspace).unwrap(),
            updated_at: Utc::now(),
        })
        .unwrap();
    let runtime = ListenerRuntimeAdapter::new(store);
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
    let status = runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();
    assert_eq!(status.state, ListenerRuntimeState::Running);

    let mut client = TcpStream::connect(bind_address).await.unwrap();
    client
            .write_all(
                b"POST /binary HTTP/1.1\r\nHost: preserved.test\r\nContent-Length: 4\r\n\r\n\x00\x81\xff\x7f",
            )
            .await
            .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert!(response.ends_with(b"\xff\x00ok"));
    upstream_task.await.unwrap();
    runtime.stop(listener.id).await.unwrap();
}
