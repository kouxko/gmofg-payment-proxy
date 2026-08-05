//! 动态 Reverse listener 的双端 mTLS 集成测试。

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use intercept_proxy_runtime::{
    HandshakePolicy, MessageLimits, MitmCertificateAuthority, MitmServerIdentity, PipelinePorts,
    ReverseClientIdentity, ReverseDownstreamTls, ReverseProxyConfig, ReverseProxyService,
    ReverseUpstreamTls, UpstreamSecurityEvidence, UpstreamTransportSecurity,
    tls::{ClientTlsAdapter, ServerTlsAdapter},
    transport::{ConnectionAcceptor, ConnectionContext, NoopPipelinePorts},
};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, SanType,
};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    version::TLS12,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
struct Identity {
    cert: Vec<u8>,
    key: Vec<u8>,
    ca: Vec<u8>,
}

#[derive(Debug)]
struct DynamicAuthority {
    ca_der: Vec<u8>,
    ca_key_der: Vec<u8>,
    issued_names: Mutex<Vec<String>>,
}

impl DynamicAuthority {
    fn new() -> Self {
        let (ca_der, ca_key_der) = ca("dynamic downstream CA");
        Self {
            ca_der,
            ca_key_der,
            issued_names: Mutex::new(Vec::new()),
        }
    }
}

impl MitmCertificateAuthority for DynamicAuthority {
    fn issue_server_identity(
        &self,
        authority_host: &str,
    ) -> intercept_proxy_runtime::Result<MitmServerIdentity> {
        self.issued_names
            .lock()
            .unwrap()
            .push(authority_host.to_owned());
        let ca_key = KeyPair::try_from(self.ca_key_der.as_slice()).unwrap();
        let issuer = Issuer::from_ca_cert_der(&self.ca_der.clone().into(), ca_key).unwrap();
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let mut params = CertificateParams::default();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.subject_alt_names = vec![SanType::DnsName(
            authority_host.to_owned().try_into().unwrap(),
        )];
        let certificate = params.signed_by(&key, &issuer).unwrap();
        Ok(MitmServerIdentity {
            certificate_chain_der: vec![certificate.der().to_vec(), self.ca_der.clone()],
            private_key_pkcs8_der: key.serialize_der().into(),
        })
    }
}

#[derive(Debug, Default)]
struct SecurityPorts {
    evidence: Mutex<Vec<UpstreamSecurityEvidence>>,
}

impl HandshakePolicy for SecurityPorts {}

#[async_trait]
impl PipelinePorts for SecurityPorts {
    async fn upstream_security_established(
        &self,
        _context: &ConnectionContext,
        evidence: &UpstreamSecurityEvidence,
    ) {
        self.evidence.lock().unwrap().push(evidence.clone());
    }
}

fn ca(common_name: &str) -> (Vec<u8>, Vec<u8>) {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let cert = params.self_signed(&key).unwrap();
    (cert.der().to_vec(), key.serialize_der())
}

fn identity(common_name: &str, client: bool) -> Identity {
    let (ca_der, ca_key_der) = ca(&format!("{common_name} CA"));
    let ca_key = KeyPair::try_from(ca_key_der.as_slice()).unwrap();
    let issuer = Issuer::from_ca_cert_der(&ca_der.clone().into(), ca_key).unwrap();
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![if client {
        ExtendedKeyUsagePurpose::ClientAuth
    } else {
        ExtendedKeyUsagePurpose::ServerAuth
    }];
    params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
    let cert = params.signed_by(&key, &issuer).unwrap();
    Identity {
        cert: cert.der().to_vec(),
        key: key.serialize_der(),
        ca: ca_der,
    }
}

fn context(peer_port: u16) -> ConnectionContext {
    ConnectionContext {
        runtime_epoch: Uuid::new_v4(),
        connection_id: Uuid::new_v4(),
        channel: intercept_proxy_runtime::ChannelId::new("reverse-test").unwrap(),
        peer_addr: (Ipv4Addr::LOCALHOST, peer_port).into(),
        accepted_at: std::time::SystemTime::now(),
        tls_peer: None,
    }
}

#[tokio::test]
async fn reverse_listener_negotiates_mtls_on_both_sides_and_preserves_body_bytes() {
    let downstream_server = identity("reverse-server", false);
    let downstream_client = identity("application-client", true);
    let upstream_server = identity("upstream-server", false);
    let upstream_client = identity("reverse-upstream-client", true);

    let upstream_tls = ServerTlsAdapter::build(
        vec![upstream_server.cert.clone(), upstream_server.ca.clone()],
        upstream_server.key.clone(),
        upstream_client.ca.clone(),
        None,
        Arc::new(NoopPipelinePorts),
    )
    .unwrap();
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (tcp, peer) = upstream_listener.accept().await.unwrap();
        let mut accepted = upstream_tls
            .accept(Box::new(tcp), &context(peer.port()))
            .await
            .unwrap();
        assert!(accepted.tls_peer.take().is_some());
        let mut request = [0_u8; 8];
        accepted.io.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"\x00request");
        accepted.io.write_all(b"\x81response\xff").await.unwrap();
        accepted.io.shutdown().await.unwrap();
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        upstream_origin: format!("https://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: Some(ReverseDownstreamTls {
            server_identity: ReverseClientIdentity {
                certificate_chain_der: vec![
                    downstream_server.cert.clone(),
                    downstream_server.ca.clone(),
                ],
                private_key_pkcs8_der: downstream_server.key.clone().into(),
            },
            dynamic_server_identity: None,
            dynamic_server_name_allowlist: Vec::new(),
            client_trust_der: vec![downstream_client.ca.clone()],
            client_authentication_required: true,
        }),
        upstream_tls: Some(ReverseUpstreamTls {
            server_trust_der: vec![upstream_server.ca.clone()],
            client_identity: Some(ReverseClientIdentity {
                certificate_chain_der: vec![
                    upstream_client.cert.clone(),
                    upstream_client.ca.clone(),
                ],
                private_key_pkcs8_der: upstream_client.key.clone().into(),
            }),
            verify_hostname: true,
        }),
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    let downstream_tls = ClientTlsAdapter::build(
        vec![downstream_client.cert, downstream_client.ca],
        downstream_client.key,
        downstream_server.ca,
    )
    .unwrap();
    let tcp = TcpStream::connect(reverse_address).await.unwrap();
    let mut client = downstream_tls
        .connect("127.0.0.1", Box::new(tcp))
        .await
        .unwrap();
    client.write_all(b"\x00request").await.unwrap();
    let mut response = [0_u8; 10];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(&response, b"\x81response\xff");

    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn reverse_listener_without_explicit_identity_issues_leaf_for_client_sni() {
    let authority = Arc::new(DynamicAuthority::new());
    let fallback = identity("fallback-server", false);
    let downstream_client = identity("ordinary-client", true);
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream_listener.accept().await.unwrap();
        let mut request = [0_u8; 7];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"request");
        stream.write_all(b"response").await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        upstream_origin: format!("http://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: Some(ReverseDownstreamTls {
            server_identity: ReverseClientIdentity {
                certificate_chain_der: vec![fallback.cert, fallback.ca],
                private_key_pkcs8_der: fallback.key.into(),
            },
            dynamic_server_identity: Some(authority.clone()),
            dynamic_server_name_allowlist: vec!["https.gmo-fg.net".into()],
            client_trust_der: Vec::new(),
            client_authentication_required: false,
        }),
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    let client = ClientTlsAdapter::build(
        vec![downstream_client.cert, downstream_client.ca],
        downstream_client.key,
        authority.ca_der.clone(),
    )
    .unwrap();
    let tcp = TcpStream::connect(reverse_address).await.unwrap();
    let mut stream = client
        .connect("https.gmo-fg.net", Box::new(tcp))
        .await
        .unwrap();
    stream.write_all(b"request").await.unwrap();
    let mut response = [0_u8; 8];
    stream.read_exact(&mut response).await.unwrap();
    assert_eq!(&response, b"response");

    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    assert_eq!(
        authority.issued_names.lock().unwrap().as_slice(),
        ["https.gmo-fg.net"]
    );
}

#[tokio::test]
async fn reverse_listener_rejects_sni_outside_dynamic_allowlist() {
    let authority = Arc::new(DynamicAuthority::new());
    let fallback = identity("fallback-server", false);
    let downstream_client = identity("ordinary-client", true);
    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let cancellation = CancellationToken::new();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        upstream_origin: "http://127.0.0.1:9".into(),
        downstream_tls: Some(ReverseDownstreamTls {
            server_identity: ReverseClientIdentity {
                certificate_chain_der: vec![fallback.cert, fallback.ca],
                private_key_pkcs8_der: fallback.key.into(),
            },
            dynamic_server_identity: Some(authority.clone()),
            dynamic_server_name_allowlist: vec!["allowed.example.test".into()],
            client_trust_der: Vec::new(),
            client_authentication_required: false,
        }),
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    let client = ClientTlsAdapter::build(
        vec![downstream_client.cert, downstream_client.ca],
        downstream_client.key,
        authority.ca_der.clone(),
    )
    .unwrap();
    let tcp = TcpStream::connect(reverse_address).await.unwrap();
    assert!(
        client
            .connect("blocked.example.test", Box::new(tcp))
            .await
            .is_err()
    );

    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    assert!(authority.issued_names.lock().unwrap().is_empty());
}

#[tokio::test]
async fn upstream_tls_test_uses_listener_ca_hostname_and_client_identity() {
    let upstream_server = identity("probe-upstream-server", false);
    let upstream_client = identity("probe-upstream-client", true);
    let upstream_tls = ServerTlsAdapter::build(
        vec![upstream_server.cert.clone(), upstream_server.ca.clone()],
        upstream_server.key.clone(),
        upstream_client.ca.clone(),
        None,
        Arc::new(NoopPipelinePorts),
    )
    .unwrap();
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (tcp, peer) = upstream_listener.accept().await.unwrap();
        let accepted = upstream_tls
            .accept(Box::new(tcp), &context(peer.port()))
            .await
            .unwrap();
        assert!(accepted.tls_peer.is_some(), "Server 必须收到客户端证书");
    });

    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: (Ipv4Addr::LOCALHOST, 0).into(),
        upstream_origin: format!("https://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: None,
        upstream_tls: Some(ReverseUpstreamTls {
            server_trust_der: vec![upstream_server.ca],
            client_identity: Some(ReverseClientIdentity {
                certificate_chain_der: vec![upstream_client.cert, upstream_client.ca],
                private_key_pkcs8_der: upstream_client.key.into(),
            }),
            verify_hostname: true,
        }),
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();

    let result = service.test_upstream_tls().await.unwrap();
    assert_eq!(result.resolved_address, upstream_address);
    assert_eq!(result.tls_version, "TLS 1.2");
    assert!(result.cipher_suite.starts_with("TLS_"));
    assert!(result.peer_subject.contains("probe-upstream-server"));
    assert!(!result.peer_sha256_fingerprint.is_empty());
    assert!(result.hostname_verification_enabled);
    assert!(result.client_identity_configured);
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn upstream_tls_test_supports_ordinary_tls_without_a_client_identity() {
    let upstream_server = identity("ordinary-tls-server", false);
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS12])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![
                    CertificateDer::from(upstream_server.cert.clone()),
                    CertificateDer::from(upstream_server.ca.clone()),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(upstream_server.key)),
            )
            .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (tcp, _) = upstream_listener.accept().await.unwrap();
        acceptor.accept(tcp).await.unwrap();
    });

    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: (Ipv4Addr::LOCALHOST, 0).into(),
        upstream_origin: format!("https://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: None,
        upstream_tls: Some(ReverseUpstreamTls {
            server_trust_der: vec![upstream_server.ca],
            client_identity: None,
            verify_hostname: true,
        }),
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap();

    let result = service.test_upstream_tls().await.unwrap();
    assert_eq!(result.tls_version, "TLS 1.2");
    assert!(!result.client_identity_configured);
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn reverse_http_request_reports_ordinary_tls_evidence_to_pipeline() {
    let upstream_server = identity("ordinary-request-server", false);
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS12])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![
                    CertificateDer::from(upstream_server.cert.clone()),
                    CertificateDer::from(upstream_server.ca.clone()),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(upstream_server.key)),
            )
            .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let expected_host = format!("Host: 127.0.0.1:{}\r\n", upstream_address.port());
    let upstream_task = tokio::spawn(async move {
        let (tcp, _) = upstream_listener.accept().await.unwrap();
        let mut stream = acceptor.accept(tcp).await.unwrap();
        let mut request = [0_u8; 256];
        let read = stream.read(&mut request).await.unwrap();
        assert!(request[..read].starts_with(b"GET /probe HTTP/1.1\r\n"));
        assert!(
            String::from_utf8_lossy(&request[..read]).contains(&expected_host),
            "固定上游必须收到按配置 authority 改写后的 Host"
        );
        assert!(
            !request[..read]
                .windows(b"Host: client.invalid".len())
                .any(|window| window == b"Host: client.invalid"),
            "客户端原始 Host 不得泄漏到固定上游"
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let ports = Arc::new(SecurityPorts::default());
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        upstream_origin: format!("https://127.0.0.1:{}", upstream_address.port()),
        downstream_tls: None,
        upstream_tls: Some(ReverseUpstreamTls {
            server_trust_der: vec![upstream_server.ca],
            client_identity: None,
            verify_hostname: true,
        }),
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap()
    .with_pipeline(
        intercept_proxy_runtime::ChannelId::new("ordinary-tls").unwrap(),
        ports.clone(),
        MessageLimits::default(),
        4,
    )
    .unwrap();
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    let mut client = TcpStream::connect(reverse_address).await.unwrap();
    client
        .write_all(b"GET /probe HTTP/1.1\r\nHost: client.invalid\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));

    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
    let evidence = ports.evidence.lock().unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].transport, UpstreamTransportSecurity::Tls);
    assert_eq!(evidence[0].tls_version.as_deref(), Some("TLS 1.2"));
    assert!(
        evidence[0]
            .peer_subject
            .as_deref()
            .unwrap()
            .contains("ordinary-request-server")
    );
    assert_eq!(evidence[0].hostname_verification_enabled, Some(true));
    assert!(!evidence[0].client_identity_configured);
    assert!(!evidence[0].client_identity_submitted);
}

#[tokio::test]
async fn reverse_http_request_rewrites_ipv6_authority_host() {
    let upstream_listener = TcpListener::bind((Ipv6Addr::LOCALHOST, 0))
        .await
        .expect("测试环境必须支持 IPv6 loopback");
    let upstream_address = upstream_listener.local_addr().unwrap();
    let expected_host = format!("Host: [::1]:{}\r\n", upstream_address.port());
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream_listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0_u8; 1];
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        let request = String::from_utf8(request).unwrap();
        assert!(
            request.contains(&expected_host),
            "IPv6 固定上游必须收到带方括号和非默认端口的 Host，实际请求：{request:?}"
        );
        assert!(!request.contains("Host: client.invalid"));
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();
    });

    let reverse_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let reverse_address = reverse_listener.local_addr().unwrap();
    let service = ReverseProxyService::build(ReverseProxyConfig {
        bind_addr: reverse_address,
        upstream_origin: format!("http://[::1]:{}", upstream_address.port()),
        downstream_tls: None,
        upstream_tls: None,
        connect_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        write_timeout: Duration::from_secs(2),
    })
    .await
    .unwrap()
    .with_pipeline(
        intercept_proxy_runtime::ChannelId::new("ipv6-host-rewrite").unwrap(),
        Arc::new(NoopPipelinePorts),
        MessageLimits::default(),
        4,
    )
    .unwrap();
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let reverse_task = tokio::spawn(async move {
        service
            .serve_listener(reverse_listener, task_cancellation)
            .await
    });

    let mut client = TcpStream::connect(reverse_address).await.unwrap();
    client
        .write_all(b"GET /ipv6 HTTP/1.1\r\nHost: client.invalid\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));

    upstream_task.await.unwrap();
    cancellation.cancel();
    reverse_task.await.unwrap().unwrap();
}
