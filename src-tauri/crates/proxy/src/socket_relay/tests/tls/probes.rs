use super::*;

#[tokio::test]
async fn upstream_mtls_uses_the_configured_client_identity() {
    let target = identity("mtls target", false);
    let client = identity("proxy client", true);
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let target_for_server = target.clone();
    let trusted_ca = client.ca.clone();
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        let stream = mtls_accept(stream, &target_for_server, &trusted_ca)
            .await
            .unwrap();
        echo_after_eof(stream, Arc::new(b"upstream-mtls".to_vec())).await;
    });
    let bind_addr = reserve_address();
    let service = Arc::new(
        SocketRelayService::build(base_config(
            bind_addr,
            upstream_address,
            SocketRelaySecurity::TcpToTls {
                upstream_tls: SocketUpstreamTlsConfig {
                    server_trust_der: vec![target.ca.clone()],
                    client_identity: Some(socket_identity(&client)),
                    verify_hostname: true,
                    tls_server_name: None,
                },
            },
        ))
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let server_cancel = cancellation.clone();
    let running = Arc::clone(&service);
    let server = tokio::spawn(async move { running.serve(server_cancel).await });
    let stream = connect_retry(bind_addr).await;
    roundtrip_payload(stream, b"upstream-mtls").await;
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn upstream_mtls_probe_fails_closed_without_client_identity() {
    let target = identity("mtls probe target", false);
    let required_client = identity("required client", true);
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let target_for_server = target.clone();
    let trusted_ca = required_client.ca.clone();
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        assert!(
            mtls_accept(stream, &target_for_server, &trusted_ca)
                .await
                .is_err()
        );
    });
    let service = SocketRelayService::build(base_config(
        reserve_address(),
        upstream_address,
        SocketRelaySecurity::TcpToTls {
            upstream_tls: SocketUpstreamTlsConfig {
                server_trust_der: vec![target.ca.clone()],
                client_identity: None,
                verify_hostname: true,
                tls_server_name: None,
            },
        },
    ))
    .unwrap();
    let error = service.test_upstream_connection().await.unwrap_err();
    assert_eq!(error.code, "SOCKET_UPSTREAM_TLS_FAILED");
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn upstream_tls_automatically_negotiates_static_rsa_aes_gcm() {
    let target = rsa_identity("legacy rsa target");
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let target_for_server = target.clone();
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        let stream = static_rsa_accept(stream, &target_for_server).await;
        drop(stream);
    });
    let service = SocketRelayService::build(base_config(
        reserve_address(),
        upstream_address,
        SocketRelaySecurity::TcpToTls {
            upstream_tls: SocketUpstreamTlsConfig {
                server_trust_der: vec![target.ca.clone()],
                client_identity: None,
                verify_hostname: true,
                tls_server_name: None,
            },
        },
    ))
    .unwrap();
    let result = service.test_upstream_connection().await.unwrap();
    let tls = result.tls.expect("TLS evidence must be reported");
    assert_eq!(tls.tls_version, "TLS 1.2");
    assert!(matches!(
        tls.cipher_suite.as_str(),
        "AES256-GCM-SHA384" | "AES128-GCM-SHA256"
    ));
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn upstream_tls_automatically_negotiates_modern_tls13() {
    let target = identity("modern tls13 target", false);
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let target_for_server = target.clone();
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        let stream = tls13_accept(stream, &target_for_server).await;
        drop(stream);
    });
    let service = SocketRelayService::build(base_config(
        reserve_address(),
        upstream_address,
        SocketRelaySecurity::TcpToTls {
            upstream_tls: SocketUpstreamTlsConfig {
                server_trust_der: vec![target.ca.clone()],
                client_identity: None,
                verify_hostname: true,
                tls_server_name: None,
            },
        },
    ))
    .unwrap();
    let result = service.test_upstream_connection().await.unwrap();
    let tls = result.tls.expect("TLS evidence must be reported");
    assert_eq!(tls.tls_version, "TLS 1.3");
    assert!(tls.cipher_suite.starts_with("TLS_AES_"));
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn upstream_tls_connects_to_an_ip_using_an_explicit_server_name() {
    let target = dns_identity("payments.example.test");
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let target_for_server = target.clone();
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        let stream = tls13_accept(stream, &target_for_server).await;
        drop(stream);
    });
    let service = SocketRelayService::build(base_config(
        reserve_address(),
        upstream_address,
        SocketRelaySecurity::TcpToTls {
            upstream_tls: SocketUpstreamTlsConfig {
                server_trust_der: vec![target.ca.clone()],
                client_identity: None,
                verify_hostname: true,
                tls_server_name: Some("payments.example.test".into()),
            },
        },
    ))
    .unwrap();
    let result = service.test_upstream_connection().await.unwrap();
    assert!(result.tls.unwrap().hostname_verification_enabled);
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn upstream_tls_probe_discovers_dns_names_without_reporting_strict_verification() {
    let target = dns_identity("payments.example.test");
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let target_for_server = target.clone();
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        let stream = tls13_accept(stream, &target_for_server).await;
        drop(stream);
    });
    let service = SocketRelayService::build(base_config(
        reserve_address(),
        upstream_address,
        SocketRelaySecurity::TcpToTls {
            upstream_tls: SocketUpstreamTlsConfig {
                server_trust_der: vec![target.ca.clone()],
                client_identity: None,
                verify_hostname: true,
                tls_server_name: None,
            },
        },
    ))
    .unwrap();
    let result = service.test_upstream_connection().await.unwrap();
    assert_eq!(
        result.tls_server_name_candidates,
        vec!["payments.example.test"]
    );
    assert!(!result.tls.unwrap().hostname_verification_enabled);
    upstream_task.await.unwrap();
}
