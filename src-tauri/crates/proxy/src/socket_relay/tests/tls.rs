use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, SanType,
};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    version::TLS12,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
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
            },
        },
    ))
    .unwrap();
    let error = service.test_upstream_connection().await.unwrap_err();
    assert_eq!(error.code, "SOCKET_UPSTREAM_TLS_FAILED");
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

fn socket_identity(identity: &Identity) -> SocketTlsIdentity {
    SocketTlsIdentity {
        certificate_chain_der: vec![identity.cert.clone(), identity.ca.clone()],
        private_key_pkcs8_der: identity.key.clone().into(),
    }
}
