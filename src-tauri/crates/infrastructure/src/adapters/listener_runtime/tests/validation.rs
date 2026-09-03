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
