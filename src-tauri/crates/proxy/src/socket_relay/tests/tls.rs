use std::{
    net::{IpAddr, Ipv4Addr},
    pin::Pin,
    sync::Arc,
};

use openssl::{
    pkey::PKey,
    rsa::Rsa,
    ssl::{Ssl, SslAcceptor, SslMethod, SslVersion},
    x509::X509,
};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, PKCS_RSA_SHA256, SanType,
};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    version::{TLS12, TLS13},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_openssl::SslStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_util::sync::CancellationToken;

use super::{
    SocketEndpoint, SocketRelayConfig, SocketRelayService, connect_retry, reserve_address,
};
use crate::socket_relay::{
    SocketDownstreamTlsConfig, SocketRelaySecurity, SocketTlsIdentity, SocketUpstreamTlsConfig,
};

#[derive(Clone)]
struct Identity {
    cert: Vec<u8>,
    key: Vec<u8>,
    ca: Vec<u8>,
}

#[derive(Clone, Copy)]
enum BridgeMode {
    TcpToTls,
    TlsToTcp,
    TlsToTls,
}

#[tokio::test]
async fn all_tls_bridge_modes_preserve_binary_and_half_close() {
    for mode in [
        BridgeMode::TcpToTls,
        BridgeMode::TlsToTcp,
        BridgeMode::TlsToTls,
    ] {
        bridge_roundtrip(mode).await;
    }
}

#[tokio::test]
async fn required_downstream_mtls_rejects_missing_then_accepts_trusted_client() {
    let proxy = identity("mtls proxy", false);
    let client = identity("trusted client", true);
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream.accept().await.unwrap();
        echo_after_eof(stream, Arc::new(b"trusted-payload".to_vec())).await;
    });
    let bind_addr = reserve_address();
    let service = Arc::new(
        SocketRelayService::build(base_config(
            bind_addr,
            upstream_address,
            SocketRelaySecurity::TlsToTcp {
                downstream_tls: SocketDownstreamTlsConfig {
                    server_identity: socket_identity(&proxy),
                    client_trust_der: vec![client.ca.clone()],
                    client_authentication_required: true,
                },
            },
        ))
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let server_cancel = cancellation.clone();
    let running = Arc::clone(&service);
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let missing = connect_retry(bind_addr).await;
    assert!(tls_connect_result(missing, &proxy.ca, None).await.is_err());
    let trusted = connect_retry(bind_addr).await;
    let trusted = tls_connect_result(trusted, &proxy.ca, Some(&client))
        .await
        .unwrap();
    roundtrip_payload(trusted, b"trusted-payload").await;
    cancellation.cancel();
    server.await.unwrap().unwrap();
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
        let mut stream = static_rsa_accept(stream, &target_for_server).await;
        stream.shutdown().await.unwrap();
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
        let mut stream = tls13_accept(stream, &target_for_server).await;
        stream.shutdown().await.unwrap();
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
        let mut stream = tls13_accept(stream, &target_for_server).await;
        stream.shutdown().await.unwrap();
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
        let mut stream = tls13_accept(stream, &target_for_server).await;
        stream.shutdown().await.unwrap();
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

fn base_config(
    bind_addr: std::net::SocketAddr,
    upstream: std::net::SocketAddr,
    security: SocketRelaySecurity,
) -> SocketRelayConfig {
    SocketRelayConfig {
        bind_addr,
        allowed_client_cidrs: Vec::new(),
        upstream: SocketEndpoint {
            host: "127.0.0.1".into(),
            port: upstream.port(),
        },
        security,
        maximum_connections: 4,
        connect_timeout: std::time::Duration::from_secs(2),
        read_timeout: std::time::Duration::from_secs(2),
        write_timeout: std::time::Duration::from_secs(2),
    }
}

async fn bridge_roundtrip(mode: BridgeMode) {
    let downstream_tls = matches!(mode, BridgeMode::TlsToTcp | BridgeMode::TlsToTls);
    let upstream_tls = matches!(mode, BridgeMode::TcpToTls | BridgeMode::TlsToTls);
    let proxy_identity = identity("socket proxy", false);
    let target_identity = identity("socket target", false);
    let upstream_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream_listener.local_addr().unwrap();
    let payload = Arc::new(
        (0..35_000_u32)
            .map(|index| index.wrapping_mul(67).to_le_bytes()[0])
            .collect::<Vec<_>>(),
    );
    let expected = Arc::clone(&payload);
    let target_for_server = target_identity.clone();
    let upstream_task = tokio::spawn(async move {
        let (stream, _) = upstream_listener.accept().await.unwrap();
        if upstream_tls {
            let stream = tls_accept(stream, &target_for_server).await;
            echo_after_eof(stream, expected).await;
        } else {
            echo_after_eof(stream, expected).await;
        }
    });

    let bind_addr = reserve_address();
    let downstream_config = SocketDownstreamTlsConfig {
        server_identity: socket_identity(&proxy_identity),
        client_trust_der: Vec::new(),
        client_authentication_required: false,
    };
    let upstream_config = SocketUpstreamTlsConfig {
        server_trust_der: vec![target_identity.ca.clone()],
        client_identity: None,
        verify_hostname: true,
        tls_server_name: None,
    };
    let security = match mode {
        BridgeMode::TcpToTls => SocketRelaySecurity::TcpToTls {
            upstream_tls: upstream_config,
        },
        BridgeMode::TlsToTcp => SocketRelaySecurity::TlsToTcp {
            downstream_tls: downstream_config,
        },
        BridgeMode::TlsToTls => SocketRelaySecurity::TlsToTls {
            downstream_tls: downstream_config,
            upstream_tls: upstream_config,
        },
    };
    let service = Arc::new(
        SocketRelayService::build(SocketRelayConfig {
            bind_addr,
            allowed_client_cidrs: Vec::new(),
            upstream: SocketEndpoint {
                host: "127.0.0.1".into(),
                port: upstream_address.port(),
            },
            security,
            maximum_connections: 4,
            connect_timeout: std::time::Duration::from_secs(2),
            read_timeout: std::time::Duration::from_secs(2),
            write_timeout: std::time::Duration::from_secs(2),
        })
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let server_cancel = cancellation.clone();
    let running = Arc::clone(&service);
    let server = tokio::spawn(async move { running.serve(server_cancel).await });
    let stream = connect_retry(bind_addr).await;
    if downstream_tls {
        let stream = tls_connect(stream, &proxy_identity.ca).await;
        roundtrip(stream, &payload).await;
    } else {
        roundtrip(stream, &payload).await;
    }
    cancellation.cancel();
    server.await.unwrap().unwrap();
    upstream_task.await.unwrap();
}

async fn echo_after_eof<S>(mut stream: S, expected: Arc<Vec<u8>>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut received = Vec::new();
    stream.read_to_end(&mut received).await.unwrap();
    assert_eq!(received, *expected);
    stream.write_all(b"bridge-reply\0\xff").await.unwrap();
    stream.shutdown().await.unwrap();
}

async fn roundtrip<S>(mut stream: S, payload: &[u8])
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream.write_all(payload).await.unwrap();
    stream.shutdown().await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, b"bridge-reply\0\xff");
}

async fn roundtrip_payload<S>(mut stream: S, payload: &[u8])
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream.write_all(payload).await.unwrap();
    stream.shutdown().await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, b"bridge-reply\0\xff");
}

async fn tls_accept(
    stream: TcpStream,
    identity: &Identity,
) -> tokio_rustls::server::TlsStream<TcpStream> {
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS12])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![
                    CertificateDer::from(identity.cert.clone()),
                    CertificateDer::from(identity.ca.clone()),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key.clone())),
            )
            .unwrap();
    TlsAcceptor::from(Arc::new(config))
        .accept(stream)
        .await
        .unwrap()
}

async fn tls13_accept(
    stream: TcpStream,
    identity: &Identity,
) -> tokio_rustls::server::TlsStream<TcpStream> {
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![
                    CertificateDer::from(identity.cert.clone()),
                    CertificateDer::from(identity.ca.clone()),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key.clone())),
            )
            .unwrap();
    TlsAcceptor::from(Arc::new(config))
        .accept(stream)
        .await
        .unwrap()
}

async fn tls_connect(stream: TcpStream, ca: &[u8]) -> tokio_rustls::client::TlsStream<TcpStream> {
    tls_connect_result(stream, ca, None).await.unwrap()
}

async fn tls_connect_result(
    stream: TcpStream,
    ca: &[u8],
    identity: Option<&Identity>,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, std::io::Error> {
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(ca.to_vec())).unwrap();
    let builder =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS12])
            .unwrap()
            .with_root_certificates(roots);
    let config = if let Some(identity) = identity {
        builder
            .with_client_auth_cert(
                vec![
                    CertificateDer::from(identity.cert.clone()),
                    CertificateDer::from(identity.ca.clone()),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key.clone())),
            )
            .unwrap()
    } else {
        builder.with_no_client_auth()
    };
    TlsConnector::from(Arc::new(config))
        .connect(ServerName::IpAddress(Ipv4Addr::LOCALHOST.into()), stream)
        .await
}

async fn mtls_accept(
    stream: TcpStream,
    identity: &Identity,
    client_ca: &[u8],
) -> Result<tokio_rustls::server::TlsStream<TcpStream>, std::io::Error> {
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(client_ca.to_vec())).unwrap();
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .unwrap();
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS12])
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![
                    CertificateDer::from(identity.cert.clone()),
                    CertificateDer::from(identity.ca.clone()),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key.clone())),
            )
            .unwrap();
    TlsAcceptor::from(Arc::new(config)).accept(stream).await
}

async fn static_rsa_accept(stream: TcpStream, identity: &Identity) -> SslStream<TcpStream> {
    let mut builder = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).unwrap();
    builder
        .set_min_proto_version(Some(SslVersion::TLS1_2))
        .unwrap();
    builder
        .set_max_proto_version(Some(SslVersion::TLS1_2))
        .unwrap();
    builder
        .set_cipher_list("AES256-GCM-SHA384:AES128-GCM-SHA256")
        .unwrap();
    builder
        .set_certificate(&X509::from_der(&identity.cert).unwrap())
        .unwrap();
    builder
        .add_extra_chain_cert(X509::from_der(&identity.ca).unwrap())
        .unwrap();
    builder
        .set_private_key(&PKey::private_key_from_pkcs8(&identity.key).unwrap())
        .unwrap();
    builder.check_private_key().unwrap();
    let acceptor = builder.build();
    let ssl = Ssl::new(acceptor.context()).unwrap();
    let mut stream = SslStream::new(ssl, stream).unwrap();
    Pin::new(&mut stream).accept().await.unwrap();
    stream
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

fn dns_identity(server_name: &str) -> Identity {
    let (ca_der, ca_key_der) = ca(&format!("{server_name} CA"));
    let ca_key = KeyPair::try_from(ca_key_der.as_slice()).unwrap();
    let issuer = Issuer::from_ca_cert_der(&ca_der.clone().into(), ca_key).unwrap();
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.subject_alt_names = vec![SanType::DnsName(server_name.try_into().unwrap())];
    let cert = params.signed_by(&key, &issuer).unwrap();
    Identity {
        cert: cert.der().to_vec(),
        key: key.serialize_der(),
        ca: ca_der,
    }
}

fn rsa_identity(common_name: &str) -> Identity {
    let ca_private_key = PKey::from_rsa(Rsa::generate(2_048).unwrap()).unwrap();
    let ca_key_der = ca_private_key.private_key_to_pkcs8().unwrap();
    let ca_key = KeyPair::from_pkcs8_der_and_sign_algo(
        &PrivatePkcs8KeyDer::from(ca_key_der.as_slice()),
        &PKCS_RSA_SHA256,
    )
    .unwrap();
    let mut ca_params = CertificateParams::default();
    let mut ca_dn = DistinguishedName::new();
    ca_dn.push(DnType::CommonName, format!("{common_name} CA"));
    ca_params.distinguished_name = ca_dn;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca = ca_params.self_signed(&ca_key).unwrap();

    let issuer = Issuer::from_ca_cert_der(ca.der(), ca_key).unwrap();
    let leaf_private_key = PKey::from_rsa(Rsa::generate(2_048).unwrap()).unwrap();
    let leaf_key_der = leaf_private_key.private_key_to_pkcs8().unwrap();
    let leaf_key = KeyPair::from_pkcs8_der_and_sign_algo(
        &PrivatePkcs8KeyDer::from(leaf_key_der.as_slice()),
        &PKCS_RSA_SHA256,
    )
    .unwrap();
    let mut leaf_params = CertificateParams::default();
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    leaf_params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
    let leaf = leaf_params.signed_by(&leaf_key, &issuer).unwrap();
    Identity {
        cert: leaf.der().to_vec(),
        key: leaf_key.serialize_der(),
        ca: ca.der().to_vec(),
    }
}

fn socket_identity(identity: &Identity) -> SocketTlsIdentity {
    SocketTlsIdentity {
        certificate_chain_der: vec![identity.cert.clone(), identity.ca.clone()],
        private_key_pkcs8_der: identity.key.clone().into(),
    }
}
