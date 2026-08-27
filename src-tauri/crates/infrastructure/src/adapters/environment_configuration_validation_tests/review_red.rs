use super::*;

#[tokio::test]
async fn rejects_more_than_sixteen_upstream_addresses_before_connecting() {
    let targets = (10_000_u16..10_017)
        .map(|port| ("127.0.0.1".to_owned(), port))
        .collect();

    let error = adapter(Arc::new(ProjectionPorts::default()))
        .probe_dns_tcp(targets)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "DTO_LIMIT_EXCEEDED");
}

#[tokio::test]
async fn mtls_probe_rejects_a_missing_client_certificate() {
    let fixture = MtlsFixture::new();
    let server = rejecting_mtls_server(
        fixture.listener,
        fixture.server_leaf.certificate_der.clone(),
        fixture.server_leaf.private_key_pkcs8_der.to_vec(),
        fixture.trusted_client_root,
    );
    let service = build_tls_probe(TlsProbeInput {
        host: "127.0.0.1".into(),
        port: fixture.port,
        server_name: Some("localhost".into()),
        server_trust_der: vec![fixture.server_root.certificate_der.clone()],
        client_identity: None,
        verify_hostname: true,
    })
    .unwrap();

    assert!(service.test_upstream_connection().await.is_err());
    let observed = server.await.unwrap();
    assert!(!observed.authenticated);
    assert_eq!(observed.application_bytes, 0);
}

#[tokio::test]
async fn mtls_probe_rejects_a_client_certificate_from_an_untrusted_root() {
    let fixture = MtlsFixture::new();
    let (wrong_client_pem, _, _) = client_identity_pem();
    let wrong_client = CertificateService
        .parse_client_identity_pem(&wrong_client_pem)
        .unwrap();
    let server = rejecting_mtls_server(
        fixture.listener,
        fixture.server_leaf.certificate_der.clone(),
        fixture.server_leaf.private_key_pkcs8_der.to_vec(),
        fixture.trusted_client_root,
    );
    let service = build_tls_probe(TlsProbeInput {
        host: "127.0.0.1".into(),
        port: fixture.port,
        server_name: Some("localhost".into()),
        server_trust_der: vec![fixture.server_root.certificate_der.clone()],
        client_identity: Some(SocketTlsIdentity {
            certificate_chain_der: wrong_client.certificate_chain_der.clone(),
            private_key_pkcs8_der: wrong_client.private_key_pkcs8_der.clone(),
        }),
        verify_hostname: true,
    })
    .unwrap();

    assert!(service.test_upstream_connection().await.is_err());
    let observed = server.await.unwrap();
    assert!(!observed.authenticated);
    assert_eq!(observed.application_bytes, 0);
}

struct MtlsFixture {
    server_root: crate::CertificateBundle,
    server_leaf: crate::CertificateBundle,
    trusted_client_root: Vec<u8>,
    listener: StdTcpListener,
    port: u16,
}

impl MtlsFixture {
    fn new() -> Self {
        let server_root = CertificateService
            .generate_root_ca("Negative mTLS Server Root")
            .unwrap();
        let server_leaf = CertificateService
            .generate_leaf(
                &server_root.certificate_der,
                &server_root.private_key_pkcs8_der,
                &LeafCertificateRequest {
                    common_name: "localhost".into(),
                    dns_names: vec!["localhost".into()],
                    ip_addresses: Vec::new(),
                },
            )
            .unwrap();
        let (_, _, trusted_client_root) = client_identity_pem();
        let listener = StdTcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        Self {
            server_root,
            server_leaf,
            trusted_client_root,
            listener,
            port,
        }
    }
}

struct MtlsServerObservation {
    authenticated: bool,
    application_bytes: usize,
}

fn rejecting_mtls_server(
    listener: StdTcpListener,
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
    client_root: Vec<u8>,
) -> tokio::task::JoinHandle<MtlsServerObservation> {
    tokio::task::spawn_blocking(move || {
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(client_root)).unwrap();
        let config = ServerConfig::builder()
            .with_client_cert_verifier(
                WebPkiClientVerifier::builder(Arc::new(roots))
                    .build()
                    .unwrap(),
            )
            .with_single_cert(
                vec![CertificateDer::from(certificate_der)],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key_der)),
            )
            .unwrap();
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut connection = ServerConnection::new(Arc::new(config)).unwrap();
        let mut application_bytes = 0;
        while connection.is_handshaking() {
            if connection.complete_io(&mut socket).is_err() {
                application_bytes += drain_application_bytes(&mut connection);
                return MtlsServerObservation {
                    authenticated: false,
                    application_bytes,
                };
            }
            application_bytes += drain_application_bytes(&mut connection);
        }
        application_bytes += drain_application_bytes(&mut connection);
        MtlsServerObservation {
            authenticated: true,
            application_bytes,
        }
    })
}

fn drain_application_bytes(connection: &mut ServerConnection) -> usize {
    let mut total = 0;
    let mut buffer = [0_u8; 1024];
    loop {
        match connection.reader().read(&mut buffer) {
            Ok(0) | Err(_) => return total,
            Ok(read) => total += read,
        }
    }
}
