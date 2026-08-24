//! TLS 1.2 双向认证和监听生命周期的集成测试。
//!
//! 测试使用临时证书与本机端口证明客户端/服务端身份校验、拒绝路径和取消清理；它不能
//! 替代正式 macOS App 防火墙、Windows 安装包或真实 GMO-FG 证书链验收。

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use intercept_proxy_runtime::{
    ChannelConfig, ChannelId, ConnectionAdmission, ConnectionContext, FaultAction, HandshakePolicy,
    MessageLimits, ProxyConfig, ProxyError, ProxySupervisor, Result, SystemClock, TlsPeerIdentity,
    TokioListenerBinder, UpstreamConnector,
    http::{ConnectionService, ForwardRequest, NoopPipelinePorts, PipelinePorts},
    tls::{ClientTlsAdapter, ServerTlsAdapter},
    transport::{AcceptedConnection, BoxIo, ConnectionAcceptor},
};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, SanType,
};
use ring::digest::{SHA256, digest};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    version::{TLS12, TLS13},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Notify,
};
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct Identity {
    cert: Vec<u8>,
    key: Vec<u8>,
    ca: Vec<u8>,
}

fn channel_id(value: &str) -> ChannelId {
    ChannelId::new(value).expect("valid test channel ID")
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

fn identity(common_name: &str, san: &str, client: bool) -> Identity {
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
    params.subject_alt_names = vec![SanType::DnsName(san.try_into().unwrap())];
    let cert = params.signed_by(&key, &issuer).unwrap();
    Identity {
        cert: cert.der().to_vec(),
        key: key.serialize_der(),
        ca: ca_der,
    }
}

fn context() -> ConnectionContext {
    ConnectionContext {
        runtime_epoch: Uuid::new_v4(),
        connection_id: Uuid::new_v4(),
        channel: channel_id("alpha"),
        peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 45_000),
        accepted_at: SystemTime::now(),
        tls_peer: None,
    }
}

async fn listener() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    (listener, address)
}

#[derive(Debug)]
struct SignalingAcceptor {
    inner: ServerTlsAdapter,
    entered: Arc<Notify>,
}

#[async_trait]
impl ConnectionAcceptor for SignalingAcceptor {
    async fn accept(&self, io: BoxIo, context: &ConnectionContext) -> Result<AcceptedConnection> {
        self.entered.notify_one();
        self.inner.accept(io, context).await
    }
}

#[derive(Debug)]
struct UnusedUpstream;

#[async_trait]
impl UpstreamConnector for UnusedUpstream {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        _request: ForwardRequest,
        _actions: &[FaultAction],
        _informational: Option<&intercept_proxy_runtime::http::InformationalResponseSink>,
        _cancellation: &CancellationToken,
    ) -> Result<intercept_proxy_runtime::http::UpstreamExchange> {
        unreachable!("silent TLS clients never reach the upstream connector")
    }
}

#[tokio::test]
async fn valid_mtls_negotiates_tls12_and_exposes_peer_identity() {
    let server = identity("proxy.local", "localhost", false);
    let client = identity("alpha-client", "alpha-client", true);
    let server_tls = ServerTlsAdapter::build(
        vec![server.cert.clone(), server.ca.clone()],
        server.key,
        client.ca.clone(),
        None,
        Arc::new(NoopPipelinePorts),
    )
    .unwrap();
    let client_tls =
        ClientTlsAdapter::build(vec![client.cert, client.ca], client.key, server.ca).unwrap();
    let (listener, address) = listener().await;
    let server_task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        server_tls.accept(Box::new(tcp) as BoxIo, &context()).await
    });
    let tcp = TcpStream::connect(address).await.unwrap();
    let connected = client_tls
        .connect_with_evidence("localhost", Box::new(tcp))
        .await
        .unwrap();
    assert_eq!(connected.evidence.tls_version, "TLS 1.2");
    assert!(connected.evidence.cipher_suite.starts_with("TLS_"));
    assert!(
        connected
            .evidence
            .peer
            .subject_summary
            .contains("proxy.local")
    );
    assert!(connected.evidence.hostname_verification_enabled);
    assert!(connected.evidence.client_identity_configured);
    assert!(connected.evidence.client_identity_submitted);
    let accepted = server_task.await.unwrap().unwrap();
    assert!(
        accepted
            .tls_peer
            .unwrap()
            .subject_summary
            .contains("alpha-client")
    );
}

#[tokio::test]
async fn wrong_ca_and_hostname_mismatch_are_rejected() {
    let server = identity("proxy.local", "localhost", false);
    let client = identity("alpha-client", "alpha-client", true);
    let wrong_ca = ca("Wrong CA").0;
    for (trusted_ca, host) in [(wrong_ca, "localhost"), (server.ca.clone(), "wrong.local")] {
        let server_tls = ServerTlsAdapter::build(
            vec![server.cert.clone(), server.ca.clone()],
            server.key.clone(),
            client.ca.clone(),
            None,
            Arc::new(NoopPipelinePorts),
        )
        .unwrap();
        let client_tls = ClientTlsAdapter::build(
            vec![client.cert.clone(), client.ca.clone()],
            client.key.clone(),
            trusted_ca,
        )
        .unwrap();
        let (listener, address) = listener().await;
        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            server_tls.accept(Box::new(tcp) as BoxIo, &context()).await
        });
        let tcp = TcpStream::connect(address).await.unwrap();
        assert!(client_tls.connect(host, Box::new(tcp)).await.is_err());
        assert!(server_task.await.unwrap().is_err());
    }
}

#[tokio::test]
async fn missing_client_certificate_and_tls13_only_client_are_rejected() {
    let server = identity("proxy.local", "localhost", false);
    let client = identity("alpha-client", "alpha-client", true);
    for (version, with_client_auth) in [(&TLS12, false), (&TLS13, true)] {
        let server_tls = ServerTlsAdapter::build(
            vec![server.cert.clone(), server.ca.clone()],
            server.key.clone(),
            client.ca.clone(),
            None,
            Arc::new(NoopPipelinePorts),
        )
        .unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(server.ca.clone())).unwrap();
        let builder =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[version])
                .unwrap()
                .with_root_certificates(roots);
        let config = if with_client_auth {
            builder
                .with_client_auth_cert(
                    vec![
                        CertificateDer::from(client.cert.clone()),
                        CertificateDer::from(client.ca.clone()),
                    ],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(client.key.clone())),
                )
                .unwrap()
        } else {
            builder.with_no_client_auth()
        };
        let (listener, address) = listener().await;
        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            server_tls.accept(Box::new(tcp) as BoxIo, &context()).await
        });
        let tcp = TcpStream::connect(address).await.unwrap();
        let connector = TlsConnector::from(Arc::new(config));
        assert!(
            connector
                .connect(ServerName::try_from("localhost").unwrap(), tcp)
                .await
                .is_err()
        );
        assert!(server_task.await.unwrap().is_err());
    }
}

#[derive(Debug)]
struct RejectPolicy {
    called: Arc<AtomicBool>,
}

impl HandshakePolicy for RejectPolicy {
    fn reject_tls_handshake(&self, _: &ConnectionContext, _: &TlsPeerIdentity) -> Result<bool> {
        self.called.store(true, Ordering::SeqCst);
        Ok(true)
    }
}

#[tokio::test]
async fn fingerprint_and_policy_reject_before_http_handler_can_open() {
    let server = identity("proxy.local", "localhost", false);
    let client = identity("alpha-client", "alpha-client", true);
    for (pin, reject_policy) in [
        (Some(vec![0; 32]), false),
        (Some(digest(&SHA256, &client.cert).as_ref().to_vec()), true),
    ] {
        let policy_called = Arc::new(AtomicBool::new(false));
        let handler_opened = Arc::new(AtomicBool::new(false));
        let policy: Arc<dyn HandshakePolicy> = if reject_policy {
            Arc::new(RejectPolicy {
                called: Arc::clone(&policy_called),
            })
        } else {
            Arc::new(NoopPipelinePorts)
        };
        let server_tls = ServerTlsAdapter::build(
            vec![server.cert.clone(), server.ca.clone()],
            server.key.clone(),
            client.ca.clone(),
            pin,
            policy,
        )
        .unwrap();
        let client_tls = ClientTlsAdapter::build(
            vec![client.cert.clone(), client.ca.clone()],
            client.key.clone(),
            server.ca.clone(),
        )
        .unwrap();
        let (listener, address) = listener().await;
        let opened = Arc::clone(&handler_opened);
        let server_task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let result = server_tls.accept(Box::new(tcp) as BoxIo, &context()).await;
            if result.is_ok() {
                opened.store(true, Ordering::SeqCst);
            }
            result
        });
        let tcp = TcpStream::connect(address).await.unwrap();
        let client_result = tokio::time::timeout(
            Duration::from_secs(2),
            client_tls.connect("localhost", Box::new(tcp)),
        )
        .await
        .unwrap();
        assert!(client_result.is_err());
        let error: ProxyError = server_task.await.unwrap().unwrap_err();
        assert_eq!(error.code, "TLS_HANDSHAKE_FAILED");
        assert!(!handler_opened.load(Ordering::SeqCst));
        assert_eq!(policy_called.load(Ordering::SeqCst), reject_policy);
    }
}

#[tokio::test]
async fn stop_cancels_a_silent_inbound_tls_handshake() {
    let server = identity("proxy.local", "localhost", false);
    let client = identity("alpha-client", "alpha-client", true);
    let entered = Arc::new(Notify::new());
    let acceptor = SignalingAcceptor {
        inner: ServerTlsAdapter::build(
            vec![server.cert, server.ca],
            server.key,
            client.ca,
            None,
            Arc::new(NoopPipelinePorts),
        )
        .unwrap(),
        entered: Arc::clone(&entered),
    };
    let service = ConnectionService {
        acceptor: Arc::new(acceptor),
        upstream: Arc::new(UnusedUpstream),
        ports: Arc::new(NoopPipelinePorts),
        capabilities: Arc::new(intercept_proxy_runtime::PlainHttpCapabilityFactory::new(
            "tls-test-workspace",
            "tls-test-listener",
        )),
        endpoint: "unused.test:443".into(),
        clock: Arc::new(SystemClock),
        admission: ConnectionAdmission::new(4).unwrap(),
        allowed_client_cidrs: Vec::new(),
        limits: MessageLimits::default(),
        read_timeout: Duration::from_secs(30),
        write_timeout: Duration::from_secs(30),
    };
    let supervisor = ProxySupervisor::new(Arc::new(TokioListenerBinder), service);
    let started = supervisor
        .start(ProxyConfig {
            channels: vec![ChannelConfig {
                channel: channel_id("alpha"),
                enabled: true,
                listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                upstream_url: "https://upstream.test/".into(),
            }],
            limits: MessageLimits::default(),
            max_connections: 4,
            connect_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(30),
            rewrite_host: true,
            leaf_sans: vec!["localhost".into()],
        })
        .await
        .unwrap();

    let _silent = TcpStream::connect(started.listeners[&channel_id("alpha")])
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .expect("connection entered the rustls handshake");

    let stopped = tokio::time::timeout(Duration::from_secs(1), supervisor.stop())
        .await
        .expect("stop cancels the silent TLS handshake")
        .unwrap();
    assert_eq!(stopped.state, intercept_proxy_runtime::ProxyState::Stopped);
}

#[tokio::test]
async fn silent_inbound_tls_handshake_times_out_and_releases_its_permit() {
    let server = identity("proxy.local", "localhost", false);
    let client = identity("alpha-client", "alpha-client", true);
    let client_tls = ClientTlsAdapter::build(
        vec![client.cert.clone(), client.ca.clone()],
        client.key.clone(),
        server.ca.clone(),
    )
    .unwrap();
    let entered = Arc::new(Notify::new());
    let acceptor = SignalingAcceptor {
        inner: ServerTlsAdapter::build(
            vec![server.cert, server.ca],
            server.key,
            client.ca,
            None,
            Arc::new(NoopPipelinePorts),
        )
        .unwrap(),
        entered: Arc::clone(&entered),
    };
    let service = ConnectionService {
        acceptor: Arc::new(acceptor),
        upstream: Arc::new(UnusedUpstream),
        ports: Arc::new(NoopPipelinePorts),
        capabilities: Arc::new(intercept_proxy_runtime::PlainHttpCapabilityFactory::new(
            "tls-test-workspace",
            "tls-timeout-listener",
        )),
        endpoint: "unused.test:443".into(),
        clock: Arc::new(SystemClock),
        admission: ConnectionAdmission::new(1).unwrap(),
        allowed_client_cidrs: Vec::new(),
        limits: MessageLimits::default(),
        read_timeout: Duration::from_millis(30),
        write_timeout: Duration::from_secs(30),
    };
    let supervisor = ProxySupervisor::new(Arc::new(TokioListenerBinder), service);
    let started = supervisor
        .start(ProxyConfig {
            channels: vec![ChannelConfig {
                channel: channel_id("alpha"),
                enabled: true,
                listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                upstream_url: "https://upstream.test/".into(),
            }],
            limits: MessageLimits::default(),
            max_connections: 1,
            connect_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_millis(30),
            rewrite_host: true,
            leaf_sans: vec!["localhost".into()],
        })
        .await
        .unwrap();
    let address = started.listeners[&channel_id("alpha")];

    let silent = TcpStream::connect(address).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .expect("connection entered the rustls handshake");
    let mut byte = [0_u8; 1];
    let closed = tokio::time::timeout(Duration::from_secs(1), silent.readable())
        .await
        .expect("silent TLS handshake reaches its read timeout");
    closed.unwrap();
    assert_eq!(silent.try_read(&mut byte).unwrap_or(0), 0);

    let tcp = TcpStream::connect(address).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        client_tls.connect("localhost", Box::new(tcp)),
    )
    .await
    .expect("the handshake permit was released after timeout")
    .unwrap();

    supervisor.stop().await.unwrap();
}
