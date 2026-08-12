#[tokio::test]
async fn fixed_server_connect_cannot_escape_to_request_authority() {
    let fixed_upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixed_upstream_address = fixed_upstream.local_addr().unwrap();
    let fixed_upstream_task = tokio::spawn(async move {
        if let Ok(Ok((mut stream, _))) =
            tokio::time::timeout(Duration::from_secs(2), fixed_upstream.accept()).await
        {
            let mut request = [0_u8; 512];
            let _ = stream.read(&mut request).await;
            let _ = stream
                    .write_all(
                        b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
        }
    });
    let forbidden_target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let forbidden_address = forbidden_target.local_addr().unwrap();

    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind_address = reservation.local_addr().unwrap();
    drop(reservation);
    let listener = ProxyListener {
        id: ListenerId::new(),
        name: "fixed CONNECT isolation".into(),
        bind_address: bind_address.ip().to_string(),
        port: bind_address.port(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            fixed_server: Some(FixedServerSettings {
                upstream_url: format!("http://{fixed_upstream_address}"),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let runtime = ListenerRuntimeAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
    runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();

    let mut client = TcpStream::connect(bind_address).await.unwrap();
    client
        .write_all(
            format!("CONNECT {forbidden_address} HTTP/1.1\r\nHost: {forbidden_address}\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = [0_u8; 256];
    let _ = tokio::time::timeout(Duration::from_secs(2), client.read(&mut response)).await;

    assert!(
        tokio::time::timeout(Duration::from_millis(300), forbidden_target.accept())
            .await
            .is_err(),
        "固定 Server 模式不得按 CONNECT authority 建立旁路隧道"
    );
    runtime.stop(listener.id).await.unwrap();
    fixed_upstream_task.await.unwrap();
}

#[tokio::test]
async fn multiple_fixed_server_listeners_route_to_their_own_upstream_origins() {
    async fn upstream(response_body: &'static [u8]) -> (SocketAddr, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 256];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0, "request headers ended unexpectedly");
                request.extend_from_slice(&buffer[..read]);
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(response_body).await.unwrap();
        });
        (address, task)
    }

    async fn reserve_local_address() -> SocketAddr {
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        address
    }

    async fn request(address: SocketAddr) -> Vec<u8> {
        let mut client = TcpStream::connect(address).await.unwrap();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: local.test\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        response
    }

    let (transaction_upstream, transaction_task) = upstream(b"transaction-response").await;
    let (webhook_upstream, webhook_task) = upstream(b"webhook-response").await;
    let transaction_bind = reserve_local_address().await;
    let webhook_bind = reserve_local_address().await;
    let fixed = |name: &str, bind: SocketAddr, upstream: SocketAddr| ProxyListener {
        id: ListenerId::new(),
        name: name.into(),
        enabled: false,
        bind_address: bind.ip().to_string(),
        port: bind.port(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            fixed_server: Some(FixedServerSettings {
                upstream_url: format!("http://{upstream}"),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let transaction = fixed("Transaction", transaction_bind, transaction_upstream);
    let webhook = fixed("Webhook API", webhook_bind, webhook_upstream);
    let workspace = ProxyWorkspace {
        id: intercept_proxy_domain::WorkspaceId::new(),
        name: "multiple mappings".into(),
        revision: Revision::INITIAL,
        listeners: vec![transaction.clone(), webhook.clone()],
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
            value: encode_workspace_record(&workspace).unwrap(),
            updated_at: Utc::now(),
        })
        .unwrap();
    let runtime = ListenerRuntimeAdapter::new(store);
    runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));

    for listener in [transaction.clone(), webhook.clone()] {
        runtime.start(workspace.clone(), listener).await.unwrap();
    }
    assert_eq!(runtime.statuses().await.unwrap().len(), 2);

    let transaction_response = request(transaction_bind).await;
    let webhook_response = request(webhook_bind).await;
    assert!(transaction_response.ends_with(b"transaction-response"));
    assert!(webhook_response.ends_with(b"webhook-response"));

    transaction_task.await.unwrap();
    webhook_task.await.unwrap();
    runtime.stop(transaction.id).await.unwrap();
    runtime.stop(webhook.id).await.unwrap();
}
