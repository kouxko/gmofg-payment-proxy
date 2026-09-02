use super::*;

#[tokio::test]
async fn upstream_tls_bundle_supplies_an_intermediate_omitted_by_the_server() {
    let (target, intermediate, root) = intermediate_signed_identity("bundle target");
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let target_for_server = target.clone();
    let upstream_task = tokio::spawn(async move {
        let (root_only, _) = upstream.accept().await.unwrap();
        assert!(
            tls13_accept_leaf_only(root_only, &target_for_server)
                .await
                .is_err()
        );
        let (bundle, _) = upstream.accept().await.unwrap();
        tls13_accept_leaf_only(bundle, &target_for_server)
            .await
            .unwrap();
    });

    let root_only = upstream_probe(upstream_address, vec![root.clone()]);
    assert!(root_only.test_upstream_connection().await.is_err());

    let bundle = upstream_probe(upstream_address, vec![intermediate, root]);
    let result = bundle.test_upstream_connection().await.unwrap();
    assert_eq!(result.tls.unwrap().tls_version, "TLS 1.3");
    upstream_task.await.unwrap();
}

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
    let (listener, bind_addr) = bind_listener().await;
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
    let server = tokio::spawn(async move {
        running
            .serve_listener(listener, uuid::Uuid::new_v4(), server_cancel)
            .await
    });
    let stream = connect_retry(bind_addr).await;
    roundtrip_payload(stream, b"upstream-mtls").await;
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.await.unwrap();
}

fn upstream_probe(
    upstream_address: std::net::SocketAddr,
    server_trust_der: Vec<Vec<u8>>,
) -> SocketRelayService {
    SocketRelayService::build(base_config(
        reserve_address(),
        upstream_address,
        SocketRelaySecurity::TcpToTls {
            upstream_tls: SocketUpstreamTlsConfig {
                server_trust_der,
                client_identity: None,
                verify_hostname: true,
                tls_server_name: None,
            },
        },
    ))
    .unwrap()
}

fn intermediate_signed_identity(common_name: &str) -> (Identity, Vec<u8>, Vec<u8>) {
    let (root_der, root_key_der) = ca(&format!("{common_name} Root"));
    let root_key = KeyPair::try_from(root_key_der.as_slice()).unwrap();
    let root_issuer = Issuer::from_ca_cert_der(&root_der.clone().into(), root_key).unwrap();

    let intermediate_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut intermediate_params = CertificateParams::default();
    let mut intermediate_name = DistinguishedName::new();
    intermediate_name.push(DnType::CommonName, format!("{common_name} Intermediate"));
    intermediate_params.distinguished_name = intermediate_name;
    intermediate_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    intermediate_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let intermediate = intermediate_params
        .signed_by(&intermediate_key, &root_issuer)
        .unwrap();
    let intermediate_der = intermediate.der().to_vec();
    let intermediate_issuer =
        Issuer::from_ca_cert_der(intermediate.der(), intermediate_key).unwrap();

    let leaf_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut leaf_params = CertificateParams::default();
    let mut leaf_name = DistinguishedName::new();
    leaf_name.push(DnType::CommonName, common_name);
    leaf_params.distinguished_name = leaf_name;
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf_params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
    let leaf = leaf_params
        .signed_by(&leaf_key, &intermediate_issuer)
        .unwrap();
    (
        Identity {
            cert: leaf.der().to_vec(),
            key: leaf_key.serialize_der(),
            ca: intermediate_der.clone(),
        },
        intermediate_der,
        root_der,
    )
}

async fn tls13_accept_leaf_only(
    stream: TcpStream,
    identity: &Identity,
) -> Result<tokio_rustls::server::TlsStream<TcpStream>, std::io::Error> {
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(identity.cert.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key.clone())),
            )
            .unwrap();
    TlsAcceptor::from(Arc::new(config)).accept(stream).await
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
