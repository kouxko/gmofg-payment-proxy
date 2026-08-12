use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use intercept_proxy_runtime::{
    ForwardAuthenticationMode, ForwardProxyConfig, ForwardProxyService, HandshakePolicy,
    MessageLimits, MitmCertificateAuthority, MitmServerIdentity, NoAuthentication, PipelinePorts,
    ReverseClientIdentity, ReverseDownstreamTls, ReverseProxyConfig, ReverseProxyService,
    ReverseUpstreamTls, UpstreamScheme, UpstreamSecurityEvidence, UpstreamTransport,
    UpstreamTransportSecurity,
    tls::{ClientTlsAdapter, ServerTlsAdapter},
    http::NoopPipelinePorts,
    transport::{ConnectionAcceptor, ConnectionContext},
};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, SanType,
};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig, SignatureScheme,
    crypto::WebPkiSupportedAlgorithms,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    version::TLS12,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
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

/// 构造只向服务端声明指定签名算法的 TLS 1.2 客户端。
///
/// Android 设备使用 Conscrypt 生成 ClientHello。真实设备如果没有声明
/// ECDSA，动态 ECDSA 叶子证书就无法完成握手。该辅助函数让集成测试不依赖
/// 真机也能稳定覆盖这一兼容性边界，同时保留完整证书链验证算法，避免把
/// “证书无法验证”和“握手签名算法不匹配”混为一谈。
fn tls12_connector_with_signature_schemes(
    root_der: Vec<u8>,
    schemes: &[SignatureScheme],
) -> TlsConnector {
    let mut provider = rustls::crypto::ring::default_provider();
    let supported = provider.signature_verification_algorithms;
    let filtered = supported
        .mapping
        .iter()
        .copied()
        .filter(|(scheme, _)| schemes.contains(scheme))
        .collect::<Vec<_>>();
    assert!(!filtered.is_empty(), "测试必须至少保留一种签名算法");
    provider.signature_verification_algorithms = WebPkiSupportedAlgorithms {
        all: supported.all,
        mapping: Box::leak(filtered.into_boxed_slice()),
    };

    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(root_der)).unwrap();
    let config = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&TLS12])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

async fn connect_with_signature_schemes(
    address: std::net::SocketAddr,
    server_name: &'static str,
    root_der: Vec<u8>,
    schemes: &[SignatureScheme],
) -> std::io::Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let connector = tls12_connector_with_signature_schemes(root_der, schemes);
    let tcp = TcpStream::connect(address).await?;
    let server_name = ServerName::try_from(server_name).unwrap();
    connector.connect(server_name, tcp).await
}
