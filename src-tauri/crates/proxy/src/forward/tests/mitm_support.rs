async fn read_raw_http_request_body<S>(stream: &mut S) -> Bytes
where
    S: AsyncRead + Unpin,
{
    let mut received = Vec::new();
    let header_end = loop {
        if let Some(index) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        let mut buffer = [0u8; 256];
        let read = stream.read(&mut buffer).await.unwrap();
        assert_ne!(read, 0, "request ended before its HTTP headers");
        received.extend_from_slice(&buffer[..read]);
    };
    let headers = std::str::from_utf8(&received[..header_end]).unwrap();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while received.len() - header_end < content_length {
        let mut buffer = [0u8; 256];
        let read = stream.read(&mut buffer).await.unwrap();
        assert_ne!(read, 0, "request ended before its declared body");
        received.extend_from_slice(&buffer[..read]);
    }
    Bytes::copy_from_slice(&received[header_end..header_end + content_length])
}

#[derive(Debug)]
struct StaticCertificateAuthority {
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
    issued: AtomicUsize,
}

impl MitmCertificateAuthority for StaticCertificateAuthority {
    fn issue_server_identity(&self, _authority_host: &str) -> Result<MitmServerIdentity> {
        self.issued.fetch_add(1, Ordering::SeqCst);
        Ok(MitmServerIdentity {
            certificate_chain_der: vec![self.certificate_der.clone()],
            private_key_pkcs8_der: zeroize::Zeroizing::new(self.private_key_der.clone()),
        })
    }
}

#[derive(Debug)]
struct TestTlsUpstreamConnector {
    config: Arc<ClientConfig>,
}

#[async_trait::async_trait]
impl MitmUpstreamConnector for TestTlsUpstreamConnector {
    async fn connect(
        &self,
        authority_host: &str,
        upstream: TcpStream,
        _cancellation: &CancellationToken,
    ) -> Result<BoxIo> {
        let name = ServerName::try_from(authority_host.to_owned()).unwrap();
        let stream = TlsConnector::from(self.config.clone())
            .connect(name, upstream)
            .await
            .unwrap();
        Ok(Box::new(stream))
    }
}

fn test_ca_and_leaf(host: IpAddr) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let root_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut root_params = CertificateParams::default();
    root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    root_params.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    let root = root_params.self_signed(&root_key).unwrap();
    let issuer = Issuer::from_ca_cert_der(root.der(), root_key).unwrap();
    let leaf_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut leaf_params = CertificateParams::default();
    leaf_params.subject_alt_names = vec![SanType::IpAddress(host)];
    let leaf = leaf_params.signed_by(&leaf_key, &issuer).unwrap();
    (
        root.der().to_vec(),
        leaf.der().to_vec(),
        leaf_key.serialize_der(),
    )
}

fn client_config_trusting(root_der: Vec<u8>) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(root_der)).unwrap();
    Arc::new(
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

async fn exercise_mitm_drop_response(read_upstream: bool) {
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
    let (origin_event, mut origin_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (release_origin, release_origin_rx) = oneshot::channel();
    let mut release_origin = Some(release_origin);
    let target_task = tokio::spawn(async move {
        let (stream, _) = target.accept().await.unwrap();
        let mut tls = TlsAcceptor::from(target_server_config)
            .accept(stream)
            .await
            .unwrap();
        assert_eq!(
            read_raw_http_request_body(&mut tls).await,
            Bytes::from_static(b"mitm-request")
        );
        if read_upstream {
            tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nabc")
                .await
                .unwrap();
            origin_event.send("first-response-segment").unwrap();
            release_origin_rx.await.unwrap();
            tls.write_all(b"def").await.unwrap();
        } else {
            origin_event.send("complete-request").unwrap();
            let _ = release_origin_rx.await;
            let _ = tls
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await;
        }
    });

    let certificate_authority = Arc::new(StaticCertificateAuthority {
        certificate_der: leaf_der,
        private_key_der: leaf_key_der,
        issued: AtomicUsize::new(0),
    });
    let ports = Arc::new(CapturingPipelinePorts {
        request_actions: vec![FaultAction::DropResponse { read_upstream }],
        ..Default::default()
    });
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication))
        .unwrap()
        .with_mitm(
            ForwardMitmConfig {
                authority_allowlist: vec!["127.0.0.1".into()],
                maximum_cached_leaf_certificates: 8,
            },
            certificate_authority,
            Arc::new(TestTlsUpstreamConnector {
                config: trusted_client_config.clone(),
            }),
        )
        .unwrap()
        .with_pipeline(
            ChannelId::new("mitm-drop").unwrap(),
            Uuid::new_v4(),
            ports,
            MessageLimits::default(),
        );
    let (client, proxy) = tokio::io::duplex(64 * 1024);
    let proxy_task = tokio::spawn(async move {
        service
            .serve_connection(
                Box::new(proxy),
                "127.0.0.1:45102".parse().unwrap(),
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
    let upgraded = hyper::upgrade::on(response).await.unwrap();
    let downstream_tls = TlsConnector::from(trusted_client_config)
        .connect(ServerName::IpAddress(host.into()), TokioIo::new(upgraded))
        .await
        .unwrap();
    let (mut mitm_sender, mitm_connection) =
        client_http1::handshake(TokioIo::new(downstream_tls))
            .await
            .unwrap();
    let mitm_connection_task = tokio::spawn(mitm_connection);
    let request = mitm_sender.send_request(
        Request::builder()
            .method(Method::POST)
            .uri("/drop")
            .header(HOST, target_address.to_string())
            .body(Full::new(Bytes::from_static(b"mitm-request")))
            .unwrap(),
    );
    tokio::pin!(request);
    let expected_event = if read_upstream {
        "first-response-segment"
    } else {
        "complete-request"
    };
    assert_eq!(origin_event_rx.recv().await.unwrap(), expected_event);
    if read_upstream {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut request)
                .await
                .is_err(),
            "MITM drop must wait for the complete upstream response body"
        );
        release_origin.take().unwrap().send(()).unwrap();
    }
    let result = tokio::time::timeout(Duration::from_millis(500), request)
        .await
        .expect("MITM drop must close at its configured boundary");
    assert!(result.is_err(), "MITM drop must not return a synthetic 502");
    if !read_upstream {
        let _ = release_origin.take().unwrap().send(());
    }
    drop(mitm_sender);
    let _ = mitm_connection_task.await;
    drop(sender);
    let _ = connection_task.await;
    let _ = proxy_task.await;
    target_task.await.unwrap();
}
