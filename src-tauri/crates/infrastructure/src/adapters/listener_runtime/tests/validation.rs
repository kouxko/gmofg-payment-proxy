#[tokio::test]
async fn socket_read_chunk_cap_rejects_start_and_probe_before_io() {
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    for bytes in [0, 1_048_577, u32::MAX] {
        let mut listener = socket_listener(
            occupied.local_addr().unwrap(),
            upstream.local_addr().unwrap(),
        );
        let ListenerDataPlane::Socket(settings) = &mut listener.data_plane else {
            unreachable!()
        };
        settings.runtime_limits.read_chunk_bytes = bytes;
        let workspace = ProxyWorkspace {
            listeners: vec![listener.clone()],
            ..ProxyWorkspace::default()
        };
        let start_error = runtime.start(workspace.clone(), listener.clone()).await.unwrap_err();
        let probe_error = runtime.test_upstream_connection(workspace, listener).await.unwrap_err();
        for error in [start_error, probe_error] {
            assert!(error.view_model.field_errors.contains_key(
                "listeners.0.data_plane.settings.runtime_limits.read_chunk_bytes"
            ));
        }
        assert!(runtime.statuses().await.unwrap().is_empty());
    }
    assert!(tokio::time::timeout(std::time::Duration::from_millis(50), upstream.accept()).await.is_err());
}

#[tokio::test]
async fn socket_read_chunk_cap_maximum_relays_bytes_and_releases_port() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = Vec::new();
        stream.read_to_end(&mut request).await.unwrap();
        assert_eq!(request, b"\0socket");
        stream.write_all(b"reply\xff").await.unwrap();
    });
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind = reservation.local_addr().unwrap();
    drop(reservation);
    let mut listener = socket_listener(bind, upstream_address);
    let ListenerDataPlane::Socket(settings) = &mut listener.data_plane else { unreachable!() };
    settings.runtime_limits.read_chunk_bytes = 1_048_576;
    let workspace = ProxyWorkspace { listeners: vec![listener.clone()], ..ProxyWorkspace::default() };
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    runtime.start(workspace, listener.clone()).await.unwrap();
    let mut client = TcpStream::connect(bind).await.unwrap();
    client.write_all(b"\0socket").await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(std::time::Duration::from_secs(5), client.read_to_end(&mut response)).await.unwrap().unwrap();
    assert_eq!(response, b"reply\xff");
    upstream_task.await.unwrap();
    let status = runtime.statuses().await.unwrap().pop().unwrap();
    assert_eq!((status.client_to_server_bytes, status.server_to_client_bytes), (7, 6));
    runtime.stop(listener.id).await.unwrap();
    TcpListener::bind(bind).await.expect("stopped listener releases port");
}

#[tokio::test]
async fn upstream_tls_probe_requires_a_fixed_https_server() {
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    let dynamic = ProxyListener::default();
    let dynamic_workspace = ProxyWorkspace {
        listeners: vec![dynamic.clone()],
        ..ProxyWorkspace::default()
    };
    let error = runtime
        .test_upstream_tls(dynamic_workspace, dynamic)
        .await
        .unwrap_err();
    assert_eq!(error.view_model.code, "FIXED_SERVER_NOT_CONFIGURED");

    let http = ProxyListener {
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            topology: HttpTopology::remote(Some(FixedServerSettings {
                upstream_url: "http://127.0.0.1:8080".into(),
                upstream_tls: UpstreamTlsSettings::default(),
            })),
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let http_workspace = ProxyWorkspace {
        listeners: vec![http.clone()],
        ..ProxyWorkspace::default()
    };
    let error = runtime
        .test_upstream_tls(http_workspace, http)
        .await
        .unwrap_err();
    assert_eq!(error.view_model.code, "UPSTREAM_TLS_NOT_ENABLED");
}

#[tokio::test]
async fn upstream_connection_probe_accepts_a_fixed_http_server() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let accepted = tokio::spawn(async move { upstream.accept().await.unwrap() });
    let runtime = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    let listener = ProxyListener {
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            topology: HttpTopology::remote(Some(FixedServerSettings {
                upstream_url: format!("http://{upstream_address}"),
                upstream_tls: UpstreamTlsSettings::default(),
            })),
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };

    let result = runtime
        .test_upstream_connection(workspace, listener)
        .await
        .unwrap();

    assert_eq!(result.resolved_address, upstream_address.to_string());
    assert_eq!(result.scheme, "http");
    assert_eq!(result.transport, "tcp");
    assert!(result.tls.is_none());
    accepted.await.unwrap();
}

#[test]
fn pkcs12_reference_requires_external_password_reference() {
    let error = identity_reference("pkcs12:/tmp/client.p12").unwrap_err();
    assert_eq!(error.view_model.code, "CERTIFICATE_NOT_READY");
    let (path, variable) =
        identity_reference("pkcs12:/tmp/client.p12?password_env=PROXY_TEST_PASSWORD").unwrap();
    assert_eq!(path, PathBuf::from("/tmp/client.p12"));
    assert_eq!(variable.as_deref(), Some("PROXY_TEST_PASSWORD"));
}

#[test]
fn dynamic_sni_normalization_accepts_dns_and_rejects_ip_literals() {
    assert_eq!(
        normalize_sni_pattern("*.Example.Test."),
        Some("*.example.test".into())
    );
    assert_eq!(normalize_sni_pattern("10.0.34.50"), None);
    assert_eq!(normalize_sni_pattern("2001:db8::1"), None);
}
