#[tokio::test]
async fn allowlisted_connect_is_mitm_and_preserves_unmodified_body_bytes() {
    let host = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
    let (root_der, leaf_der, leaf_key_der) = test_ca_and_leaf(host);
    let trusted_client_config = client_config_trusting(root_der);
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let target_server_config = Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(leaf_der.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key_der.clone())),
            )
            .unwrap(),
    );
    let expected_request_body = Bytes::from_static(&[0x00, 0xff, 0x81, 0x40, 0x7f]);
    let expected_response_body = Bytes::from_static(&[0xfe, 0x00, 0x82, 0xa0, 0x01]);
    let request_assertion = expected_request_body.clone();
    let response_payload = expected_response_body.clone();
    let target_task = tokio::spawn(async move {
        let (stream, _) = target.accept().await.unwrap();
        let tls = TlsAcceptor::from(target_server_config)
            .accept(stream)
            .await
            .unwrap();
        let handler = service_fn(move |request: Request<Incoming>| {
            let request_assertion = request_assertion.clone();
            let response_payload = response_payload.clone();
            async move {
                assert_eq!(request.method(), Method::POST);
                assert_eq!(request.uri(), "/binary?mode=mitm&case=1");
                let actual = request.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(actual, request_assertion);
                Ok::<_, Infallible>(Response::new(Full::new(response_payload)))
            }
        });
        server_http1::Builder::new()
            .serve_connection(TokioIo::new(tls), handler)
            .await
            .unwrap();
    });

    let certificate_authority = Arc::new(StaticCertificateAuthority {
        certificate_der: leaf_der,
        private_key_der: leaf_key_der,
        issued: AtomicUsize::new(0),
    });
    let ports = Arc::new(CapturingPipelinePorts::default());
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_mitm(
            ForwardMitmConfig {
                authority_allowlist: vec!["127.0.0.1".into()],
                maximum_cached_leaf_certificates: 8,
            },
            certificate_authority.clone(),
            Arc::new(TestTlsUpstreamConnector {
                config: trusted_client_config.clone(),
            }),
        )
        .unwrap()
        .with_pipeline(
            ChannelId::new("mitm-test").unwrap(),
            Uuid::new_v4(),
            ports.clone(),
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(64 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45002".parse().unwrap(),
                CancellationToken::new(),
            )
            .await
    });
    let (mut sender, connection) = client_http1::handshake(TokioIo::new(client)).await.unwrap();
    let connection_task = tokio::spawn(connection.with_upgrades());
    let response = sender
        .send_request(
            Request::builder()
                .method(Method::CONNECT)
                .uri(target_address.to_string())
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let upgraded = hyper::upgrade::on(response).await.unwrap();
    let downstream_tls = TlsConnector::from(trusted_client_config)
        .connect(ServerName::IpAddress(host.into()), TokioIo::new(upgraded))
        .await
        .unwrap();
    let (mut mitm_sender, mitm_connection) = client_http1::handshake(TokioIo::new(downstream_tls))
        .await
        .unwrap();
    let mitm_connection_task = tokio::spawn(mitm_connection);
    let response = mitm_sender
        .send_request(
            Request::builder()
                .method(Method::POST)
                .uri("/binary?mode=mitm&case=1")
                .header(HOST, target_address.to_string())
                .body(Full::new(expected_request_body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        expected_response_body
    );
    drop(mitm_sender);
    mitm_connection_task.await.unwrap().unwrap();
    drop(sender);
    connection_task.await.unwrap().unwrap();
    proxy_task.await.unwrap().unwrap();
    target_task.await.unwrap();
    assert_eq!(certificate_authority.issued.load(Ordering::SeqCst), 1);
    assert_eq!(ports.requests.lock().unwrap().len(), 1);
    assert_eq!(
        ports.requests.lock().unwrap()[0].body,
        expected_request_body
    );
    assert_eq!(ports.responses.lock().unwrap().len(), 1);
    assert_eq!(
        ports.responses.lock().unwrap()[0].body,
        expected_response_body
    );
}

#[tokio::test]
async fn mitm_drop_without_upstream_read_closes_after_complete_request_write() {
    exercise_mitm_drop_response(false).await;
}

#[tokio::test]
async fn mitm_drop_with_upstream_read_waits_for_segmented_response_body() {
    exercise_mitm_drop_response(true).await;
}
