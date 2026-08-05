//! 动态反向代理监听器。
//!
//! 该运行时只接收已经解析为 DER 的不可变证书快照，不知道 Workspace、SQLite、文件
//! 或系统密钥库。每个下游连接只连接配置中的固定上游 origin；TLS 在两端分别终止，
//! HTTP 字节流（包括 Header 与 Body）在未进入规则修改管线时逐字节转发。

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use http::Uri;
use ring::digest::{SHA256, digest};
use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, ServerConfig, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::{ClientHello, ResolvesServerCert, WebPkiClientVerifier},
    sign::CertifiedKey,
    version::TLS12,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use x509_parser::{extensions::GeneralName, parse_x509_certificate};
use zeroize::{Zeroize, Zeroizing};

use crate::message::MessageLimits;
use crate::supervisor::ChannelId;
use crate::tls::ClientTlsAdapter;
use crate::transport::{
    AcceptedConnection, BoxIo as TransportBoxIo, ConnectionAcceptor, ConnectionAdmission,
    ConnectionContext, ConnectionService, HyperUpstreamConnector, PipelinePorts, SystemClock,
};
use crate::{ErrorCode, MitmCertificateAuthority, ProxyError, Result};

#[derive(Clone)]
pub struct ReverseClientIdentity {
    pub certificate_chain_der: Vec<Vec<u8>>,
    pub private_key_pkcs8_der: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for ReverseClientIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReverseClientIdentity")
            .field("certificate_count", &self.certificate_chain_der.len())
            .field("private_key_pkcs8_der", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct ReverseDownstreamTls {
    pub server_identity: ReverseClientIdentity,
    /// 当监听未绑定独立服务端身份时，按客户端 TLS SNI 动态签发匹配的叶子证书。
    ///
    /// 无 SNI 的客户端仍使用 `server_identity` 作为回退。显式导入独立身份时该字段为
    /// `None`，确保 Workspace 选择的证书不会被动态签发覆盖。
    pub dynamic_server_identity: Option<Arc<dyn MitmCertificateAuthority>>,
    pub client_trust_der: Vec<Vec<u8>>,
    pub client_authentication_required: bool,
}

#[derive(Clone, Debug)]
pub struct ReverseUpstreamTls {
    pub server_trust_der: Vec<Vec<u8>>,
    pub client_identity: Option<ReverseClientIdentity>,
    pub verify_hostname: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// 使用反向代理实际运行配置完成一次上游 TLS 握手后得到的公开元数据。
///
/// 这里刻意不暴露证书 DER、客户端证书或私钥。调用者只能确认网络可达、证书链与
/// 主机名策略是否通过，以及最终协商出的 TLS 参数。
pub struct UpstreamTlsHandshakeResult {
    pub resolved_address: SocketAddr,
    pub tls_version: String,
    pub cipher_suite: String,
    pub peer_subject: String,
    pub peer_sha256_fingerprint: String,
    pub hostname_verification_enabled: bool,
    pub client_identity_configured: bool,
    pub elapsed_millis: u64,
}

#[derive(Clone, Debug)]
pub struct ReverseProxyConfig {
    pub bind_addr: SocketAddr,
    pub upstream_origin: String,
    pub downstream_tls: Option<ReverseDownstreamTls>,
    pub upstream_tls: Option<ReverseUpstreamTls>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

#[derive(Clone)]
pub struct ReverseProxyService {
    config: ReverseProxyConfig,
    endpoint: UpstreamEndpoint,
    downstream_acceptor: Option<TlsAcceptor>,
    upstream_connector: Option<ClientTlsAdapter>,
    pipeline: Option<ConnectionService>,
    pipeline_channel: Option<ChannelId>,
}

impl std::fmt::Debug for ReverseProxyService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReverseProxyService")
            .field("bind_addr", &self.config.bind_addr)
            .field("upstream_origin", &self.config.upstream_origin)
            .field("downstream_tls", &self.downstream_acceptor.is_some())
            .field("upstream_tls", &self.upstream_connector.is_some())
            .field("pipeline", &self.pipeline.is_some())
            .finish_non_exhaustive()
    }
}

impl ReverseProxyService {
    pub async fn build(mut config: ReverseProxyConfig) -> Result<Self> {
        let endpoint =
            UpstreamEndpoint::parse(&config.upstream_origin, config.connect_timeout).await?;
        let downstream_acceptor = config
            .downstream_tls
            .as_ref()
            .map(build_server_acceptor)
            .transpose()?;
        let upstream_connector = match (&config.upstream_tls, endpoint.uses_tls) {
            (Some(settings), true) => Some(build_client_connector(settings)?),
            (None, true) => Some(build_client_connector(&ReverseUpstreamTls {
                server_trust_der: Vec::new(),
                client_identity: None,
                verify_hostname: true,
            })?),
            (Some(_), false) => {
                return Err(ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "upstream TLS settings cannot be used with an http origin",
                ));
            }
            (None, false) => None,
        };
        // rustls 已经取得运行所需的私钥副本。不要再让可 Clone 的 service config
        // 长期保留原始身份材料，否则每次 clone service 都会复制敏感字节。
        config.downstream_tls = None;
        config.upstream_tls = None;
        Ok(Self {
            config,
            endpoint,
            downstream_acceptor,
            upstream_connector,
            pipeline: None,
            pipeline_channel: None,
        })
    }

    /// 真实连接固定上游并完成 TLS 握手，但不发送 HTTP 请求。
    ///
    /// 该探测复用 Listener 启动时完全相同的 DNS、系统/自定义 CA、主机名验证、TLS 1.2
    /// 和可选客户端身份配置，因此可以在启动代理前识别错误密码、错误 CA、证书用途、
    /// 主机名不匹配、Server 强制 mTLS 但未配置客户端身份等问题。
    pub async fn test_upstream_tls(&self) -> Result<UpstreamTlsHandshakeResult> {
        if !self.endpoint.uses_tls {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "upstream origin uses HTTP and has no TLS handshake to test",
            ));
        }
        let connector = self.upstream_connector.as_ref().ok_or_else(|| {
            ProxyError::new(
                ErrorCode::CertificateNotReady,
                "upstream TLS connector is not configured",
            )
        })?;
        let started = Instant::now();
        let tcp = tokio::time::timeout(
            self.config.connect_timeout,
            TcpStream::connect(self.endpoint.address),
        )
        .await
        .map_err(|_| {
            ProxyError::new(
                ErrorCode::UpstreamConnectTimeout,
                "reverse upstream TCP connection timed out",
            )
        })?
        .map_err(|error| ProxyError::io("connect reverse upstream", &error))?;
        tcp.set_nodelay(true)
            .map_err(|error| ProxyError::io("configure reverse upstream", &error))?;
        let connected = tokio::time::timeout(
            self.config.connect_timeout,
            connector.connect_with_evidence(&self.endpoint.host, Box::new(tcp)),
        )
        .await
        .map_err(|_| {
            ProxyError::new(
                ErrorCode::TlsHandshakeFailed,
                "reverse upstream TLS handshake timed out",
            )
        })?
        .map_err(|error| ProxyError::new(ErrorCode::TlsHandshakeFailed, error.to_string()))?;
        let elapsed_millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let mut io = connected.io;
        let _ = io.shutdown().await;
        Ok(UpstreamTlsHandshakeResult {
            resolved_address: self.endpoint.address,
            tls_version: connected.evidence.tls_version,
            cipher_suite: connected.evidence.cipher_suite,
            peer_subject: connected.evidence.peer.subject_summary,
            peer_sha256_fingerprint: connected.evidence.peer.sha256_fingerprint,
            hostname_verification_enabled: connected.evidence.hostname_verification_enabled,
            client_identity_configured: connected.evidence.client_identity_configured,
            elapsed_millis,
        })
    }

    /// 为动态反向监听器启用与旧 supervisor 相同的抓包、规则、断点与故障动作管线。
    ///
    /// 未调用时仍保留纯字节流模式，便于非 HTTP 协议或底层传输测试；桌面 Workspace
    /// 的 HTTP/HTTPS Reverse Listener 必须调用此方法，确保 UI 中配置的规则真实生效。
    pub fn with_pipeline(
        mut self,
        channel: ChannelId,
        ports: Arc<dyn PipelinePorts>,
        limits: MessageLimits,
        maximum_connections: usize,
    ) -> Result<Self> {
        let acceptor: Arc<dyn ConnectionAcceptor> = Arc::new(ReverseConnectionAcceptor {
            tls: self.downstream_acceptor.clone(),
        });
        let upstream_tls = self.upstream_connector.clone();
        let upstream = HyperUpstreamConnector {
            address: self.endpoint.address,
            host: self.endpoint.host.clone(),
            host_header: self.endpoint.host_header.clone(),
            // Reverse Listener 只会连接配置中的固定 origin。客户端 Host 可能仍指向
            // Proxy 的监听地址，因此必须统一改写成固定上游 authority；Forward Proxy
            // 是否改写 Host 仍由它自己的配置决定，不经过这里。
            rewrite_host: true,
            tls: upstream_tls,
            connect_timeout: self.config.connect_timeout,
            write_timeout: self.config.write_timeout,
            read_timeout: self.config.read_timeout,
            limits,
        };
        self.pipeline = Some(ConnectionService {
            acceptor,
            upstream: Arc::new(upstream),
            ports,
            clock: Arc::new(SystemClock),
            admission: ConnectionAdmission::new(maximum_connections)?,
            limits,
            read_timeout: self.config.read_timeout,
            write_timeout: self.config.write_timeout,
        });
        self.pipeline_channel = Some(channel);
        Ok(self)
    }

    pub async fn serve_listener(
        &self,
        listener: TcpListener,
        cancellation: CancellationToken,
    ) -> Result<()> {
        self.serve_listener_with_epoch(listener, Uuid::new_v4(), cancellation)
            .await
    }

    /// 使用宿主分配的共享运行 epoch 启动监听器。
    ///
    /// 一个 Workspace 可同时运行多个 Reverse Listener；它们必须共享 epoch，规则的
    /// 第 N 次命中、一次性禁用和全局容量才不会因请求在不同端口间切换而被重置。
    pub async fn serve_listener_with_epoch(
        &self,
        listener: TcpListener,
        runtime_epoch: Uuid,
        cancellation: CancellationToken,
    ) -> Result<()> {
        if let (Some(pipeline), Some(channel)) = (&self.pipeline, &self.pipeline_channel) {
            return pipeline
                .run_tcp_listener(listener, channel.clone(), runtime_epoch, cancellation)
                .await;
        }
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => break,
                accepted = listener.accept() => {
                    let (stream, _) = accepted.map_err(|error| ProxyError::io("accept reverse downstream", &error))?;
                    let service = self.clone();
                    let connection_cancellation = cancellation.child_token();
                    connections.spawn(async move {
                        service.serve_connection(stream, connection_cancellation).await
                    });
                }
                joined = connections.join_next(), if !connections.is_empty() => {
                    if let Some(Err(error)) = joined {
                        return Err(ProxyError::new(ErrorCode::Internal, format!("reverse connection task failed: {error}")));
                    }
                }
            }
        }
        cancellation.cancel();
        while connections.join_next().await.is_some() {}
        Ok(())
    }

    async fn serve_connection(
        &self,
        downstream_tcp: TcpStream,
        cancellation: CancellationToken,
    ) -> Result<()> {
        downstream_tcp
            .set_nodelay(true)
            .map_err(|error| ProxyError::io("configure reverse downstream", &error))?;
        let downstream: BoxIo = if let Some(acceptor) = &self.downstream_acceptor {
            let stream = timeout_cancel(
                self.config.connect_timeout,
                &cancellation,
                acceptor.accept(downstream_tcp),
                ErrorCode::TlsHandshakeFailed,
            )
            .await?
            .map_err(|error| ProxyError::new(ErrorCode::TlsHandshakeFailed, error.to_string()))?;
            Box::new(stream)
        } else {
            Box::new(downstream_tcp)
        };

        let upstream_tcp = timeout_cancel(
            self.config.connect_timeout,
            &cancellation,
            TcpStream::connect(self.endpoint.address),
            ErrorCode::UpstreamConnectTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("connect reverse upstream", &error))?;
        upstream_tcp
            .set_nodelay(true)
            .map_err(|error| ProxyError::io("configure reverse upstream", &error))?;
        let upstream: BoxIo = if let Some(connector) = &self.upstream_connector {
            timeout_cancel(
                self.config.connect_timeout,
                &cancellation,
                connector.connect(&self.endpoint.host, Box::new(upstream_tcp)),
                ErrorCode::TlsHandshakeFailed,
            )
            .await?
            .map_err(|error| ProxyError::new(ErrorCode::TlsHandshakeFailed, error.to_string()))?
        } else {
            Box::new(upstream_tcp)
        };

        relay_exact(
            downstream,
            upstream,
            self.config.read_timeout,
            self.config.write_timeout,
            cancellation,
        )
        .await
    }
}

#[derive(Clone)]
struct ReverseConnectionAcceptor {
    tls: Option<TlsAcceptor>,
}

impl std::fmt::Debug for ReverseConnectionAcceptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReverseConnectionAcceptor")
            .field("tls", &self.tls.is_some())
            .finish()
    }
}

#[async_trait]
impl ConnectionAcceptor for ReverseConnectionAcceptor {
    async fn accept(
        &self,
        io: TransportBoxIo,
        _context: &ConnectionContext,
    ) -> Result<AcceptedConnection> {
        let Some(acceptor) = &self.tls else {
            return Ok(AcceptedConnection { io, tls_peer: None });
        };
        let stream = acceptor
            .accept(io)
            .await
            .map_err(|error| ProxyError::new(ErrorCode::TlsHandshakeFailed, error.to_string()))?;
        let tls_peer = stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|certificates| certificates.first())
            .map(|certificate| reverse_peer_identity(certificate.as_ref()))
            .transpose()?;
        Ok(AcceptedConnection {
            io: Box::new(stream),
            tls_peer,
        })
    }
}

fn reverse_peer_identity(certificate_der: &[u8]) -> Result<crate::transport::TlsPeerIdentity> {
    let (_, certificate) = parse_x509_certificate(certificate_der).map_err(config_error)?;
    let fingerprint = digest(&SHA256, certificate_der)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    Ok(crate::transport::TlsPeerIdentity {
        sha256_fingerprint: fingerprint,
        subject_summary: certificate.subject().to_string(),
    })
}

type BoxIo = TransportBoxIo;

async fn relay_exact(
    downstream: BoxIo,
    upstream: BoxIo,
    read_timeout: Duration,
    write_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<()> {
    let (down_read, down_write) = tokio::io::split(downstream);
    let (up_read, up_write) = tokio::io::split(upstream);
    let upstream_direction = copy_exact_direction(
        down_read,
        up_write,
        read_timeout,
        write_timeout,
        cancellation.child_token(),
    );
    let downstream_direction = copy_exact_direction(
        up_read,
        down_write,
        read_timeout,
        write_timeout,
        cancellation.child_token(),
    );
    tokio::try_join!(upstream_direction, downstream_direction)?;
    Ok(())
}

async fn copy_exact_direction<R, W>(
    mut reader: R,
    mut writer: W,
    read_timeout: Duration,
    write_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0_u8; 16 * 1024];
    loop {
        let read = timeout_cancel(
            read_timeout,
            &cancellation,
            reader.read(&mut buffer),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("read reverse stream", &error))?;
        if read == 0 {
            writer
                .shutdown()
                .await
                .map_err(|error| ProxyError::io("half-close reverse stream", &error))?;
            return Ok(());
        }
        timeout_cancel(
            write_timeout,
            &cancellation,
            writer.write_all(&buffer[..read]),
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("write reverse stream", &error))?;
        timeout_cancel(
            write_timeout,
            &cancellation,
            writer.flush(),
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("flush reverse stream", &error))?;
    }
}

async fn timeout_cancel<F, T>(
    duration: Duration,
    cancellation: &CancellationToken,
    future: F,
    timeout_code: ErrorCode,
) -> Result<T>
where
    F: std::future::Future<Output = T>,
{
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(ProxyError::new(ErrorCode::ProxyStopped, "reverse listener stopped")),
        outcome = tokio::time::timeout(duration, future) => outcome.map_err(|_| ProxyError::new(timeout_code, format!("reverse I/O timed out after {} ms", duration.as_millis()))),
    }
}

fn build_server_acceptor(settings: &ReverseDownstreamTls) -> Result<TlsAcceptor> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&TLS12])
        .map_err(config_error)?;
    let config = if settings.client_trust_der.is_empty() {
        if settings.client_authentication_required {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "required downstream client authentication has no trust anchor",
            ));
        }
        builder.with_no_client_auth()
    } else {
        let roots = root_store(&settings.client_trust_der)?;
        let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider);
        let verifier = if settings.client_authentication_required {
            verifier
        } else {
            verifier.allow_unauthenticated()
        }
        .build()
        .map_err(config_error)?;
        builder.with_client_cert_verifier(verifier)
    };
    let fallback = certified_key(&settings.server_identity)?;
    let config = match &settings.dynamic_server_identity {
        Some(authority) => config.with_cert_resolver(Arc::new(DynamicServerIdentityResolver {
            authority: Arc::clone(authority),
            fallback,
            cache: Mutex::new(BTreeMap::new()),
        })),
        None => config.with_cert_resolver(Arc::new(rustls::sign::SingleCertAndKey::from(fallback))),
    };
    Ok(TlsAcceptor::from(Arc::new(config)))
}

#[derive(Debug)]
struct DynamicServerIdentityResolver {
    authority: Arc<dyn MitmCertificateAuthority>,
    fallback: Arc<CertifiedKey>,
    cache: Mutex<BTreeMap<String, Arc<CertifiedKey>>>,
}

impl ResolvesServerCert for DynamicServerIdentityResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let Some(server_name) = client_hello.server_name() else {
            return Some(Arc::clone(&self.fallback));
        };
        let cache_key = server_name.to_ascii_lowercase();
        if let Some(cached) = self.cache.lock().ok()?.get(&cache_key) {
            return Some(Arc::clone(cached));
        }

        let identity = self.authority.issue_server_identity(server_name).ok()?;
        let certified = certified_key_from_parts(
            &identity.certificate_chain_der,
            identity.private_key_pkcs8_der.to_vec(),
        )
        .ok()?;
        let certified = Arc::new(certified);
        let mut cache = self.cache.lock().ok()?;
        if let Some(cached) = cache.get(&cache_key) {
            return Some(Arc::clone(cached));
        }
        if cache.len() >= 256
            && let Some(oldest_key) = cache.keys().next().cloned()
        {
            cache.remove(&oldest_key);
        }
        cache.insert(cache_key, Arc::clone(&certified));
        Some(certified)
    }
}

fn certified_key(identity: &ReverseClientIdentity) -> Result<Arc<CertifiedKey>> {
    certified_key_from_parts(
        &identity.certificate_chain_der,
        identity.private_key_pkcs8_der.to_vec(),
    )
    .map(Arc::new)
}

fn certified_key_from_parts(
    certificate_chain_der: &[Vec<u8>],
    private_key_pkcs8_der: Vec<u8>,
) -> Result<CertifiedKey> {
    let mut private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key_pkcs8_der));
    let signing_key =
        rustls::crypto::ring::sign::any_supported_type(&private_key).map_err(config_error)?;
    private_key.zeroize();
    let certified = CertifiedKey::new(certificate_chain(certificate_chain_der), signing_key);
    certified.keys_match().map_err(config_error)?;
    Ok(certified)
}

fn build_client_connector(settings: &ReverseUpstreamTls) -> Result<ClientTlsAdapter> {
    let mut roots = root_store(&settings.server_trust_der)?;
    if settings.server_trust_der.is_empty() {
        let native = rustls_native_certs::load_native_certs();
        for certificate in native.certs {
            roots.add(certificate).map_err(config_error)?;
        }
        if roots.is_empty() {
            return Err(ProxyError::new(
                ErrorCode::CertificateNotReady,
                format!(
                    "system trust store contains no usable roots: {:?}",
                    native.errors
                ),
            ));
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let chain_verifier = if settings.verify_hostname {
        None
    } else {
        Some(
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots.clone()), provider.clone())
                .build()
                .map_err(config_error)?,
        )
    };
    let builder = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS12])
        .map_err(config_error)?
        .with_root_certificates(roots);
    let mut config = if let Some(identity) = &settings.client_identity {
        builder
            .with_client_auth_cert(
                certificate_chain(&identity.certificate_chain_der),
                private_key(&identity.private_key_pkcs8_der),
            )
            .map_err(config_error)?
    } else {
        builder.with_no_client_auth()
    };
    if let Some(inner) = chain_verifier {
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(ChainOnlyServerVerifier { inner }));
    }
    Ok(ClientTlsAdapter::from_config(
        config,
        settings.verify_hostname,
        settings.client_identity.is_some(),
    ))
}

/// 保留 `WebPKI` 的证书链、有效期、用途与签名验证，只把目标主机名替换为证书自身首个
/// DNS/IP SAN。它不会退化为接受任意证书。
#[derive(Debug)]
struct ChainOnlyServerVerifier {
    inner: Arc<dyn ServerCertVerifier>,
}

impl ServerCertVerifier for ChainOnlyServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let certificate_name = certificate_san_name(end_entity.as_ref()).map_err(|_| {
            rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            )
        })?;
        self.inner.verify_server_cert(
            end_entity,
            intermediates,
            &certificate_name,
            ocsp_response,
            now,
        )
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.inner
            .verify_tls12_signature(message, certificate, signature)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.inner
            .verify_tls13_signature(message, certificate, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn certificate_san_name(certificate_der: &[u8]) -> Result<ServerName<'static>> {
    let (_, certificate) = parse_x509_certificate(certificate_der).map_err(|error| {
        ProxyError::new(
            ErrorCode::CertificateInvalid,
            format!("upstream certificate is invalid: {error}"),
        )
    })?;
    let names = certificate
        .subject_alternative_name()
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::CertificateInvalid,
                format!("upstream certificate SAN is invalid: {error}"),
            )
        })?
        .ok_or_else(|| {
            ProxyError::new(
                ErrorCode::CertificateInvalid,
                "hostname verification can only be disabled for a certificate containing SAN",
            )
        })?;
    for name in &names.value.general_names {
        match name {
            GeneralName::DNSName(name) => {
                return ServerName::try_from((*name).to_owned()).map_err(config_error);
            }
            GeneralName::IPAddress(bytes) if bytes.len() == 4 => {
                return Ok(ServerName::IpAddress(
                    std::net::Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]).into(),
                ));
            }
            GeneralName::IPAddress(bytes) if bytes.len() == 16 => {
                let octets: [u8; 16] = (*bytes).try_into().map_err(|_| {
                    ProxyError::new(ErrorCode::CertificateInvalid, "invalid IPv6 SAN length")
                })?;
                return Ok(ServerName::IpAddress(
                    std::net::Ipv6Addr::from(octets).into(),
                ));
            }
            _ => {}
        }
    }
    Err(ProxyError::new(
        ErrorCode::CertificateInvalid,
        "upstream certificate SAN contains no DNS or IP identity",
    ))
}

fn root_store(certificates: &[Vec<u8>]) -> Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    for certificate in certificates {
        roots
            .add(CertificateDer::from(certificate.clone()))
            .map_err(config_error)?;
    }
    Ok(roots)
}

fn certificate_chain(certificates: &[Vec<u8>]) -> Vec<CertificateDer<'static>> {
    certificates
        .iter()
        .cloned()
        .map(CertificateDer::from)
        .collect()
}

fn private_key(bytes: &[u8]) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(bytes.to_vec()))
}

fn config_error(error: impl std::fmt::Display) -> ProxyError {
    ProxyError::new(ErrorCode::CertificateInvalid, error.to_string())
}

#[derive(Clone, Debug)]
struct UpstreamEndpoint {
    address: SocketAddr,
    host: String,
    host_header: String,
    uses_tls: bool,
}

impl UpstreamEndpoint {
    async fn parse(value: &str, timeout: Duration) -> Result<Self> {
        let uri = value.parse::<Uri>().map_err(|error| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("invalid reverse upstream origin: {error}"),
            )
        })?;
        let scheme = uri.scheme_str().ok_or_else(|| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                "reverse upstream scheme is missing",
            )
        })?;
        let uses_tls = match scheme {
            "http" => false,
            "https" => true,
            _ => {
                return Err(ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "reverse upstream must use http or https",
                ));
            }
        };
        if uri
            .path_and_query()
            .is_some_and(|value| value.as_str() != "/")
            || value.contains('#')
            || uri
                .authority()
                .is_some_and(|authority| authority.as_str().contains('@'))
        {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "reverse upstream must be an origin without path, query, fragment, or userinfo",
            ));
        }
        let uri_host = uri.host().ok_or_else(|| {
            ProxyError::new(ErrorCode::ConfigInvalid, "reverse upstream host is missing")
        })?;
        let host = uri_host.trim_matches(['[', ']']).to_owned();
        let port = uri.port_u16().unwrap_or(if uses_tls { 443 } else { 80 });
        let address = tokio::time::timeout(timeout, tokio::net::lookup_host((host.as_str(), port)))
            .await
            .map_err(|_| {
                ProxyError::new(
                    ErrorCode::UpstreamConnectTimeout,
                    "reverse upstream DNS resolution timed out",
                )
            })?
            .map_err(|error| {
                ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    format!("cannot resolve reverse upstream: {error}"),
                )
            })?
            .next()
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "reverse upstream resolved to no addresses",
                )
            })?;
        Ok(Self {
            address,
            host: host.clone(),
            host_header: if (uses_tls && port == 443) || (!uses_tls && port == 80) {
                if host.contains(':') {
                    format!("[{host}]")
                } else {
                    host
                }
            } else if host.contains(':') {
                format!("[{host}]:{port}")
            } else {
                format!("{host}:{port}")
            },
            uses_tls,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_identity_debug_redacts_private_key_material() {
        let identity = ReverseClientIdentity {
            certificate_chain_der: vec![vec![1, 2, 3]],
            private_key_pkcs8_der: Zeroizing::new(b"unique-private-key-material".to_vec()),
        };

        let debug = format!("{identity:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("unique-private-key-material"));
        assert!(!debug.contains("117, 110, 105, 113"));
    }

    #[tokio::test]
    async fn upstream_host_header_omits_default_ports_and_brackets_ipv6() {
        let cases = [
            ("http://127.0.0.1", "127.0.0.1"),
            ("https://127.0.0.1", "127.0.0.1"),
            ("http://127.0.0.1:8080", "127.0.0.1:8080"),
            ("http://[::1]", "[::1]"),
            ("https://[::1]", "[::1]"),
            ("https://[::1]:8443", "[::1]:8443"),
        ];

        for (origin, expected) in cases {
            let endpoint = UpstreamEndpoint::parse(origin, Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(endpoint.host_header, expected, "origin: {origin}");
        }
    }

    #[tokio::test]
    async fn plaintext_reverse_listener_preserves_every_byte() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let expected = b"POST /x HTTP/1.1\r\nHost: preserved.test\r\nX-Odd:  a  b\r\nContent-Length: 5\r\n\r\n\x00\x81abc".to_vec();
        let expected_for_server = expected.clone();
        let upstream_task = tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut actual = vec![0; expected_for_server.len()];
            stream.read_exact(&mut actual).await.unwrap();
            assert_eq!(actual, expected_for_server);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\x00\x81ok")
                .await
                .unwrap();
        });
        let downstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let downstream_address = downstream.local_addr().unwrap();
        let cancellation = CancellationToken::new();
        let service = ReverseProxyService::build(ReverseProxyConfig {
            bind_addr: downstream_address,
            upstream_origin: format!("http://{upstream_address}"),
            downstream_tls: None,
            upstream_tls: None,
            connect_timeout: Duration::from_secs(2),
            read_timeout: Duration::from_secs(2),
            write_timeout: Duration::from_secs(2),
        })
        .await
        .unwrap();
        let serve_cancellation = cancellation.clone();
        let serve =
            tokio::spawn(
                async move { service.serve_listener(downstream, serve_cancellation).await },
            );
        let mut client = TcpStream::connect(downstream_address).await.unwrap();
        client.write_all(&expected).await.unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert_eq!(
            response,
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\x00\x81ok"
        );
        upstream_task.await.unwrap();
        cancellation.cancel();
        serve.await.unwrap().unwrap();
    }
}
