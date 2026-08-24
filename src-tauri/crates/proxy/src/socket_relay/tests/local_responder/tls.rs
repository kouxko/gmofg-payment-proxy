//! `LocalResponder` App 侧 TLS 与必需 mTLS 的真实握手/回包回归测试。

use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, SanType,
};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    version::TLS12,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;

use super::{
    super::{
        super::{
            NoopSocketConnectionObserver, SocketDownstreamSecurity, SocketDownstreamTlsConfig,
            SocketTlsIdentity,
        },
        support::{
            LocalFactory, ProcessorOutcome, connect_retry, limits, local_config, reserve_address,
        },
    },
    *,
};

#[derive(Clone)]
struct Identity {
    certificate: Vec<u8>,
    private_key: Vec<u8>,
    ca: Vec<u8>,
}

#[tokio::test]
async fn tls_app_connection_receives_the_local_response() {
    let server_identity = identity("local responder server", false);
    let bind_addr = reserve_address();
    let mut config = local_config(bind_addr);
    config.security = SocketDownstreamSecurity::Tls {
        downstream_tls: SocketDownstreamTlsConfig {
            server_identity: socket_identity(&server_identity),
            client_trust_der: Vec::new(),
            client_authentication_required: false,
        },
    };
    let service = Arc::new(
        SocketRelayService::build_local_responder(
            config,
            Arc::new(LocalFactory::new(ProcessorOutcome::Transform)),
            limits(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let tcp = connect_retry(bind_addr).await;
    let mut tls = tls_connect(tcp, &server_identity.ca, None).await.unwrap();
    tls.write_all(&[1, b't']).await.unwrap();
    tls.shutdown().await.unwrap();
    let mut response = Vec::new();
    tls.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, &[1, b't']);

    cancellation.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn required_mtls_rejects_missing_and_accepts_the_trusted_app_identity() {
    let server_identity = identity("local responder mTLS server", false);
    let client_identity = identity("trusted local app", true);
    let bind_addr = reserve_address();
    let mut config = local_config(bind_addr);
    config.security = SocketDownstreamSecurity::Tls {
        downstream_tls: SocketDownstreamTlsConfig {
            server_identity: socket_identity(&server_identity),
            client_trust_der: vec![client_identity.ca.clone()],
            client_authentication_required: true,
        },
    };
    let service = Arc::new(
        SocketRelayService::build_local_responder(
            config,
            Arc::new(LocalFactory::new(ProcessorOutcome::Transform)),
            limits(),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let missing = connect_retry(bind_addr).await;
    let missing = tls_connect(missing, &server_identity.ca, None).await;
    assert!(missing.is_err(), "mTLS must reject an App without identity");

    let trusted = connect_retry(bind_addr).await;
    let mut trusted = tls_connect(trusted, &server_identity.ca, Some(&client_identity))
        .await
        .unwrap();
    trusted.write_all(&[1, b'm']).await.unwrap();
    trusted.shutdown().await.unwrap();
    let mut response = Vec::new();
    trusted.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, &[1, b'm']);

    cancellation.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn direct_tls_local_responder_echoes_binary_after_app_half_close() {
    let server_identity = identity("direct local TLS server", false);
    let bind_addr = reserve_address();
    let mut config = local_config(bind_addr);
    config.security = SocketDownstreamSecurity::Tls {
        downstream_tls: SocketDownstreamTlsConfig {
            server_identity: socket_identity(&server_identity),
            client_trust_der: Vec::new(),
            client_authentication_required: false,
        },
    };
    let service = Arc::new(
        SocketRelayService::build_local_raw_responder_with_observer(
            config,
            Arc::new(NoopSocketConnectionObserver),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });
    let tcp = connect_retry(bind_addr).await;
    let mut tls = tls_connect(tcp, &server_identity.ca, None).await.unwrap();
    let payload = [0, 0xff, 0x10, b'd', b'i', b'r', b'e', b'c', b't'];

    tls.write_all(&payload).await.unwrap();
    tls.shutdown().await.unwrap();
    let mut response = Vec::new();
    tls.read_to_end(&mut response).await.unwrap();

    assert_eq!(response, payload);
    cancellation.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn direct_mtls_local_responder_rejects_an_app_without_identity() {
    let server_identity = identity("direct local mTLS server", false);
    let client_identity = identity("direct trusted local app", true);
    let bind_addr = reserve_address();
    let mut config = local_config(bind_addr);
    config.security = SocketDownstreamSecurity::Tls {
        downstream_tls: SocketDownstreamTlsConfig {
            server_identity: socket_identity(&server_identity),
            client_trust_der: vec![client_identity.ca.clone()],
            client_authentication_required: true,
        },
    };
    let service = Arc::new(
        SocketRelayService::build_local_raw_responder_with_observer(
            config,
            Arc::new(NoopSocketConnectionObserver),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });

    let missing = connect_retry(bind_addr).await;
    assert!(
        tls_connect(missing, &server_identity.ca, None)
            .await
            .is_err()
    );
    cancellation.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn direct_mtls_local_responder_echoes_for_a_trusted_app_identity() {
    let server_identity = identity("direct trusted local mTLS server", false);
    let client_identity = identity("direct trusted local app", true);
    let bind_addr = reserve_address();
    let mut config = local_config(bind_addr);
    config.security = SocketDownstreamSecurity::Tls {
        downstream_tls: SocketDownstreamTlsConfig {
            server_identity: socket_identity(&server_identity),
            client_trust_der: vec![client_identity.ca.clone()],
            client_authentication_required: true,
        },
    };
    let service = Arc::new(
        SocketRelayService::build_local_raw_responder_with_observer(
            config,
            Arc::new(NoopSocketConnectionObserver),
        )
        .unwrap(),
    );
    let cancellation = CancellationToken::new();
    let running = Arc::clone(&service);
    let server_cancel = cancellation.clone();
    let server = tokio::spawn(async move { running.serve(server_cancel).await });
    let trusted = connect_retry(bind_addr).await;
    let mut trusted = tls_connect(trusted, &server_identity.ca, Some(&client_identity))
        .await
        .unwrap();

    trusted.write_all(b"direct-mtls").await.unwrap();
    trusted.shutdown().await.unwrap();
    let mut response = Vec::new();
    trusted.read_to_end(&mut response).await.unwrap();

    assert_eq!(response, b"direct-mtls");
    cancellation.cancel();
    server.await.unwrap().unwrap();
}

async fn tls_connect(
    stream: tokio::net::TcpStream,
    ca: &[u8],
    identity: Option<&Identity>,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, std::io::Error> {
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
                    CertificateDer::from(identity.certificate.clone()),
                    CertificateDer::from(identity.ca.clone()),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.private_key.clone())),
            )
            .unwrap()
    } else {
        builder.with_no_client_auth()
    };
    TlsConnector::from(Arc::new(config))
        .connect(ServerName::IpAddress(Ipv4Addr::LOCALHOST.into()), stream)
        .await
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
    let certificate = params.signed_by(&key, &issuer).unwrap();
    Identity {
        certificate: certificate.der().to_vec(),
        private_key: key.serialize_der(),
        ca: ca_der,
    }
}

fn ca(common_name: &str) -> (Vec<u8>, Vec<u8>) {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, common_name);
    params.distinguished_name = distinguished_name;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let certificate = params.self_signed(&key).unwrap();
    (certificate.der().to_vec(), key.serialize_der())
}

fn socket_identity(identity: &Identity) -> SocketTlsIdentity {
    SocketTlsIdentity {
        certificate_chain_der: vec![identity.certificate.clone(), identity.ca.clone()],
        private_key_pkcs8_der: identity.private_key.clone().into(),
    }
}
