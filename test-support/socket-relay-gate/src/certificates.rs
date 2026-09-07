use std::{net::Ipv4Addr, sync::Arc};

use intercept_proxy_runtime::socket_relay::SocketTlsIdentity;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, SanType,
};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    version::TLS12,
};
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};

#[derive(Clone)]
pub(crate) struct TestIdentity {
    pub(crate) cert: Vec<u8>,
    pub(crate) key: Vec<u8>,
    pub(crate) ca: Vec<u8>,
}

pub(crate) fn identity(common_name: &str) -> TestIdentity {
    let ca_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("generate CA key");
    let mut ca_params = CertificateParams::default();
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, format!("{common_name} CA"));
    ca_params.distinguished_name = distinguished_name;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_certificate = ca_params.self_signed(&ca_key).expect("sign CA");
    let ca = ca_certificate.der().to_vec();
    let issuer = Issuer::from_ca_cert_der(&ca.clone().into(), ca_key).expect("build issuer");

    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("generate identity key");
    let mut params = CertificateParams::default();
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.subject_alt_names = vec![SanType::IpAddress(Ipv4Addr::LOCALHOST.into())];
    let cert = params.signed_by(&key, &issuer).expect("sign identity");
    TestIdentity {
        cert: cert.der().to_vec(),
        key: key.serialize_der(),
        ca,
    }
}

pub(crate) fn socket_identity(identity: &TestIdentity) -> SocketTlsIdentity {
    SocketTlsIdentity {
        certificate_chain_der: vec![identity.cert.clone(), identity.ca.clone()],
        private_key_pkcs8_der: identity.key.clone().into(),
    }
}

pub(crate) async fn accept_tls(
    stream: TcpStream,
    identity: &TestIdentity,
) -> tokio_rustls::server::TlsStream<TcpStream> {
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS12])
            .expect("TLS 1.2 server")
            .with_no_client_auth()
            .with_single_cert(
                vec![
                    CertificateDer::from(identity.cert.clone()),
                    CertificateDer::from(identity.ca.clone()),
                ],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key.clone())),
            )
            .expect("server identity");
    TlsAcceptor::from(Arc::new(config))
        .accept(stream)
        .await
        .expect("accept TLS")
}

pub(crate) async fn connect_tls(
    stream: TcpStream,
    identity: &TestIdentity,
) -> tokio_rustls::client::TlsStream<TcpStream> {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(identity.ca.clone()))
        .expect("add test CA");
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&TLS12])
            .expect("TLS 1.2 client")
            .with_root_certificates(roots)
            .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
        .connect(ServerName::IpAddress(Ipv4Addr::LOCALHOST.into()), stream)
        .await
        .expect("connect TLS")
}
