use super::{
    Arc, AsyncWriteExt, BoxIo, CancellationToken, ChannelId, ClientTlsAdapter, ConnectionAcceptor,
    ConnectionAdmission, ConnectionService, DownstreamTlsAcceptor, ErrorCode,
    HyperUpstreamConnector, Instant, MessageLimits, PipelinePorts, ProxyError, Result,
    ReverseConnectionAcceptor, ReverseProxyConfig, ReverseUpstreamTls, SystemClock, TcpListener,
    TcpStream, UpstreamConnectionTestResult, UpstreamEndpoint, UpstreamScheme,
    UpstreamTlsHandshakeResult, UpstreamTransport, Uuid, build_client_connector, relay_exact,
    timeout_cancel,
};
use async_trait::async_trait;

use crate::listener::{
    ConnectionHandler, ConnectionTaskScope, ListenerConfig, ListenerSupervisor,
    NoopConnectionLifecycleObserver, PrimaryConnectionOutcome, sealed,
};
use crate::transport::{ConnectionContext, TokioBoundListener, TokioListenerBinder};

const REVERSE_LISTENER_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone)]
pub struct ReverseProxyService {
    config: ReverseProxyConfig,
    endpoint: UpstreamEndpoint,
    downstream_acceptor: Option<DownstreamTlsAcceptor>,
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
            .map(DownstreamTlsAcceptor::new)
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

    /// 真实解析并连接固定上游，但不发送 HTTP 请求。
    ///
    /// HTTP 验证 DNS/TCP；HTTPS 继续使用运行时相同的 TLS、CA、主机名与 mTLS 配置。
    pub async fn test_upstream_connection(&self) -> Result<UpstreamConnectionTestResult> {
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
        let scheme = if self.endpoint.uses_tls {
            UpstreamScheme::Https
        } else {
            UpstreamScheme::Http
        };
        let tls = if let Some(connector) = &self.upstream_connector {
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
            Some(UpstreamTlsHandshakeResult {
                resolved_address: self.endpoint.address,
                tls_version: connected.evidence.tls_version,
                cipher_suite: connected.evidence.cipher_suite,
                peer_subject: connected.evidence.peer.subject_summary,
                peer_sha256_fingerprint: connected.evidence.peer.sha256_fingerprint,
                hostname_verification_enabled: connected.evidence.hostname_verification_enabled,
                client_identity_configured: connected.evidence.client_identity_configured,
                elapsed_millis,
            })
        } else {
            let mut tcp = tcp;
            let _ = tcp.shutdown().await;
            None
        };
        let elapsed_millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(UpstreamConnectionTestResult {
            resolved_address: self.endpoint.address,
            scheme,
            transport: if tls.is_some() {
                UpstreamTransport::Tls
            } else {
                UpstreamTransport::Tcp
            },
            tls,
            elapsed_millis,
        })
    }

    /// 兼容旧调用方：仅接受 HTTPS，并返回原有平铺 TLS 证据。
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
        self.test_upstream_connection().await?.tls.ok_or_else(|| {
            ProxyError::new(
                ErrorCode::CertificateNotReady,
                "upstream TLS connector is not configured",
            )
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
        capabilities: Arc<dyn crate::http::HttpProtocolCapabilityFactory>,
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
            capabilities,
            endpoint: self.endpoint.address.to_string(),
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
        let supervisor = ListenerSupervisor::new(
            ListenerConfig {
                bind_addr: self.config.bind_addr,
                runtime_epoch,
                listener_id: ChannelId::new("reverse-relay")?,
                capacity: crate::listener::ListenerCapacity::new(
                    tokio::sync::Semaphore::MAX_PERMITS,
                )?,
                shutdown_grace: REVERSE_LISTENER_SHUTDOWN_GRACE,
            },
            Arc::new(TokioListenerBinder),
            Arc::new(SystemClock),
            Arc::new(self.clone()),
            Arc::new(NoopConnectionLifecycleObserver),
        )?;
        supervisor
            .run_bound(Arc::new(TokioBoundListener(listener)), cancellation)
            .await?
            .into_result("reverse listener stopped after a fatal lifecycle failure")
    }

    async fn serve_connection(
        &self,
        downstream_io: BoxIo,
        context: &ConnectionContext,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let downstream: BoxIo = if let Some(acceptor) = &self.downstream_acceptor {
            timeout_cancel(
                self.config.connect_timeout,
                &cancellation,
                acceptor.accept(downstream_io, context),
                ErrorCode::DownstreamTlsHandshakeFailed,
            )
            .await??
            .io
        } else {
            downstream_io
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

impl sealed::Sealed for ReverseProxyService {}

#[async_trait]
impl ConnectionHandler for ReverseProxyService {
    async fn handle(
        &self,
        io: BoxIo,
        context: ConnectionContext,
        _child_tasks: ConnectionTaskScope,
        cancellation: CancellationToken,
    ) -> PrimaryConnectionOutcome {
        self.serve_connection(io, &context, cancellation)
            .await
            .into()
    }
}
