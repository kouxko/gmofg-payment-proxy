use std::{
    io::{Read, Write},
    net::TcpListener as StdTcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, ExternalPackageApplicationPort, ExternalPackageDetailViewModel,
    ExternalPackageServiceStatusViewModel, ProtocolPackageDescriptionViewModel,
    ProtocolPackageKindViewModel, ProtocolPackageRef, ProtocolPackageSourceViewModel,
    ProtocolPackageStorePort, ProtocolPackageValidationViewModel, ProtocolPackageVersionViewModel,
};
use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageVersion};
use intercept_proxy_runtime::{SocketTlsIdentity, SocketUpstreamTransport};
use rustls::{
    RootCertStore, ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    server::WebPkiClientVerifier,
};
use tokio::{io::AsyncReadExt, net::TcpListener};

use super::super::listener_runtime::{InstallationTlsMaterial, ListenerMitmAuthorityProvider};
use super::{EnvironmentConfigurationValidationAdapter, TlsProbeInput, build_tls_probe};
use crate::{CertificateService, LeafCertificateRequest};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
};

#[derive(Debug, Default)]
struct ProjectionPorts {
    internal: Mutex<Option<ProtocolPackageVersionViewModel>>,
    external: Mutex<Option<ProtocolPackageVersionViewModel>>,
    internal_gets: AtomicUsize,
    external_gets: AtomicUsize,
}

#[async_trait]
impl ProtocolPackageStorePort for ProjectionPorts {
    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        panic!("package validation must not list")
    }

    async fn get(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        self.internal_gets.fetch_add(1, Ordering::SeqCst);
        Ok(self.internal.lock().unwrap().clone())
    }

    async fn set_enabled(&self, _: &ProtocolPackageRef, _: bool) -> AppResult<()> {
        panic!("package validation must not mutate")
    }

    async fn delete(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        panic!("package validation must not mutate")
    }
}

#[async_trait]
impl ExternalPackageApplicationPort for ProjectionPorts {
    async fn service_status(&self) -> AppResult<ExternalPackageServiceStatusViewModel> {
        panic!("package validation must not health probe")
    }

    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        panic!("package validation must not list")
    }

    async fn get(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        self.external_gets.fetch_add(1, Ordering::SeqCst);
        Ok(self.external.lock().unwrap().clone())
    }

    async fn describe(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        panic!("package validation must not call package RPC/description")
    }

    async fn detail(&self, _: &ProtocolPackageRef) -> AppResult<ExternalPackageDetailViewModel> {
        panic!("package validation must not call package RPC/detail")
    }

    async fn set_enabled(&self, _: &ProtocolPackageRef, _: bool) -> AppResult<()> {
        panic!("package validation must not mutate")
    }

    async fn disconnect(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        panic!("package validation must not disconnect")
    }

    async fn delete(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        panic!("package validation must not mutate")
    }
}

#[derive(Debug)]
struct UnusedInstallationRoot;

#[async_trait]
impl ListenerMitmAuthorityProvider for UnusedInstallationRoot {
    async fn freeze_installation_tls_material(&self) -> AppResult<InstallationTlsMaterial> {
        Err(AppError::new("UNUSED", "installation root not requested"))
    }
}

fn adapter(ports: Arc<ProjectionPorts>) -> EnvironmentConfigurationValidationAdapter {
    EnvironmentConfigurationValidationAdapter::new(
        ports.clone(),
        ports,
        Arc::new(UnusedInstallationRoot),
    )
}

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("validation-test").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn projection(
    source: ProtocolPackageSourceViewModel,
    enabled: bool,
) -> ProtocolPackageVersionViewModel {
    ProtocolPackageVersionViewModel {
        package: package(),
        name: "Validation Test".into(),
        host_api: 1,
        kind: ProtocolPackageKindViewModel::Socket,
        source,
        enabled,
        validation: ProtocolPackageValidationViewModel::Valid,
        installed_at: Utc::now(),
    }
}

#[tokio::test]
async fn package_validation_is_exact_get_only_and_rejects_offline_external() {
    let ports = Arc::new(ProjectionPorts::default());
    *ports.external.lock().unwrap() = Some(projection(
        ProtocolPackageSourceViewModel::External { online: false },
        true,
    ));

    let error = adapter(ports.clone())
        .validate_package_refs(&[package()])
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "EXTERNAL_PACKAGE_OFFLINE");
    assert_eq!(ports.internal_gets.load(Ordering::SeqCst), 1);
    assert_eq!(ports.external_gets.load(Ordering::SeqCst), 1);
}

#[test]
fn material_validation_parses_ephemeral_ca_and_server_identity_and_rejects_role_mismatch() {
    let ports = Arc::new(ProjectionPorts::default());
    let adapter = adapter(ports);
    let root = CertificateService
        .generate_root_ca("Validation Root")
        .unwrap();
    let leaf = CertificateService
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "localhost".into(),
                dns_names: vec!["localhost".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();
    let root_pem = pem("CERTIFICATE", &root.certificate_der);
    let identity_pem = format!(
        "{}{}{}",
        pem("CERTIFICATE", &leaf.certificate_der),
        pem("CERTIFICATE", &root.certificate_der),
        pem("PRIVATE KEY", &leaf.private_key_pkcs8_der),
    );

    adapter
        .validate_certificate_input("upstream_server_trust", Some("pem"), &root_pem, None)
        .unwrap();
    adapter
        .validate_certificate_input(
            "downstream_server_identity",
            Some("pem"),
            &identity_pem,
            Some(""),
        )
        .unwrap();
    let (client_identity, _, _) = client_identity_pem();
    adapter
        .validate_certificate_input(
            "upstream_client_identity",
            Some("pem"),
            std::str::from_utf8(&client_identity).unwrap(),
            None,
        )
        .unwrap();
    let error = adapter
        .validate_certificate_input(
            "upstream_client_identity",
            Some("base64_der"),
            &STANDARD.encode(&leaf.certificate_der),
            None,
        )
        .unwrap_err();
    assert_eq!(error.view_model.code, "CERTIFICATE_ROLE_MISMATCH");
}

#[tokio::test]
async fn dns_tcp_probe_connects_and_sends_zero_bytes() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut byte = [0_u8; 1];
        stream.read(&mut byte).await.unwrap()
    });

    adapter(Arc::new(ProjectionPorts::default()))
        .probe_dns_tcp(vec![("127.0.0.1".into(), port)])
        .await
        .unwrap();

    assert_eq!(accepted.await.unwrap(), 0);
}

#[tokio::test]
async fn tls_probe_completes_handshake_and_sends_no_application_bytes() {
    let root = CertificateService
        .generate_root_ca("TLS Probe Root")
        .unwrap();
    let leaf = CertificateService
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "localhost".into(),
                dns_names: vec!["localhost".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();
    let listener = StdTcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tls_server(
        listener,
        leaf.certificate_der.clone(),
        leaf.private_key_pkcs8_der.to_vec(),
        None,
    );
    let service = build_tls_probe(TlsProbeInput {
        host: "127.0.0.1".into(),
        port,
        server_name: Some("localhost".into()),
        server_trust_der: vec![root.certificate_der.clone()],
        client_identity: None,
        verify_hostname: true,
    })
    .unwrap();

    let result = service.test_upstream_connection().await.unwrap();

    assert_eq!(result.transport, SocketUpstreamTransport::Tls);
    assert!(result.tls.unwrap().hostname_verification_enabled);
    assert!(server.await.unwrap().is_empty());
}

#[tokio::test]
async fn mtls_probe_presents_the_exact_client_identity_and_sends_no_application_bytes() {
    let server_root = CertificateService
        .generate_root_ca("mTLS Server Root")
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
    let (client_pem, _, client_root) = client_identity_pem();
    let client = CertificateService
        .parse_client_identity_pem(&client_pem)
        .unwrap();
    let listener = StdTcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tls_server(
        listener,
        server_leaf.certificate_der.clone(),
        server_leaf.private_key_pkcs8_der.to_vec(),
        Some(client_root),
    );
    let service = build_tls_probe(TlsProbeInput {
        host: "127.0.0.1".into(),
        port,
        server_name: Some("localhost".into()),
        server_trust_der: vec![server_root.certificate_der.clone()],
        client_identity: Some(SocketTlsIdentity {
            certificate_chain_der: client.certificate_chain_der.clone(),
            private_key_pkcs8_der: client.private_key_pkcs8_der.clone(),
        }),
        verify_hostname: true,
    })
    .unwrap();

    assert!(
        service
            .test_upstream_connection()
            .await
            .unwrap()
            .tls
            .is_some()
    );
    assert!(server.await.unwrap().is_empty());
}

#[test]
fn verify_hostname_rejects_missing_or_ip_server_name_before_connecting() {
    for server_name in [None, Some("127.0.0.1".to_owned())] {
        let error = build_tls_probe(TlsProbeInput {
            host: "127.0.0.1".into(),
            port: 443,
            server_name,
            server_trust_der: Vec::new(),
            client_identity: None,
            verify_hostname: true,
        })
        .unwrap_err();
        assert_eq!(error.view_model.code, "VALIDATION_LAYER_FAILED");
    }
}

#[tokio::test]
async fn cancelling_stalled_tls_probe_closes_after_client_hello_without_business_payload() {
    let root = CertificateService
        .generate_root_ca("Cancelled Probe Root")
        .unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let observed = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        bytes
    });
    let service = build_tls_probe(TlsProbeInput {
        host: "127.0.0.1".into(),
        port,
        server_name: Some("localhost".into()),
        server_trust_der: vec![root.certificate_der.clone()],
        client_identity: None,
        verify_hostname: true,
    })
    .unwrap();

    assert!(
        tokio::time::timeout(
            Duration::from_millis(100),
            service.test_upstream_connection()
        )
        .await
        .is_err()
    );
    let bytes = tokio::time::timeout(Duration::from_secs(2), observed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bytes.first(), Some(&0x16));
    assert!(
        !bytes
            .windows(b"BUSINESS".len())
            .any(|window| window == b"BUSINESS")
    );
}

fn pem(label: &str, bytes: &[u8]) -> String {
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        STANDARD.encode(bytes)
    )
}

fn client_identity_pem() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let root = CertificateService
        .generate_root_ca("Validation Client Root")
        .unwrap();
    let root_key = KeyPair::from_pkcs8_der_and_sign_algo(
        &root.private_key_pkcs8_der.as_slice().into(),
        &PKCS_ECDSA_P256_SHA256,
    )
    .unwrap();
    let issuer =
        Issuer::from_ca_cert_der(&root.certificate_der.as_slice().into(), root_key).unwrap();
    let client_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Validation Client");
    params.distinguished_name = name;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let certificate = params.signed_by(&client_key, &issuer).unwrap();
    let private_key = client_key.serialize_der();
    let certificate_der = certificate.der().to_vec();
    let root_der = root.certificate_der.clone();
    let material = format!(
        "{}{}{}",
        pem("CERTIFICATE", &certificate_der),
        pem("CERTIFICATE", &root_der),
        pem("PRIVATE KEY", &private_key),
    )
    .into_bytes();
    (material, private_key, root_der)
}

fn tls_server(
    listener: StdTcpListener,
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
    client_root: Option<Vec<u8>>,
) -> tokio::task::JoinHandle<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        let builder = ServerConfig::builder();
        let builder = if let Some(client_root) = client_root {
            let mut roots = RootCertStore::empty();
            roots.add(CertificateDer::from(client_root)).unwrap();
            builder.with_client_cert_verifier(
                WebPkiClientVerifier::builder(Arc::new(roots))
                    .build()
                    .unwrap(),
            )
        } else {
            builder.with_no_client_auth()
        };
        let config = builder
            .with_single_cert(
                vec![CertificateDer::from(certificate_der)],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key_der)),
            )
            .unwrap();
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let connection = ServerConnection::new(Arc::new(config)).unwrap();
        let mut tls = StreamOwned::new(connection, socket);
        while tls.conn.is_handshaking() {
            tls.conn.complete_io(&mut tls.sock).unwrap();
        }
        let mut application_bytes = Vec::new();
        match tls.read_to_end(&mut application_bytes) {
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => panic!("read TLS application bytes: {error}"),
        }
        let _ = tls.flush();
        application_bytes
    })
}

#[path = "environment_configuration_validation_tests/review_red.rs"]
mod review_red;
