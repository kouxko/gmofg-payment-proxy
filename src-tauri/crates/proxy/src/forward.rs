//! 标准 HTTP/1.1 正向代理与 CONNECT 隧道。
//!
//! 该模块不依赖 Tauri、产品配置或证书存储。Host 负责把经过领域校验的监听配置和认证
//! 适配器注入进来；运行时负责协议语义、目标连接、背压、half-close、超时和取消。
//! HTTPS MITM 会在独立 TLS 适配层显式启用；本模块的 CONNECT 默认始终是透明隧道。

use std::collections::{HashMap, VecDeque};
#[cfg(test)]
use std::convert::Infallible;
use std::error::Error;
use std::fmt::Debug;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};

use bytes::{Buf, Bytes};
use http::header::{
    CONNECTION, HOST, HeaderName, HeaderValue, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE,
    TRAILER, TRANSFER_ENCODING, UPGRADE,
};
use http::{HeaderMap, Method, Request, Response, StatusCode, Uri};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Frame, Incoming, SizeHint};
use hyper::client::conn::http1 as client_http1;
use hyper::server::conn::http1 as server_http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinSet;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::fault::{self, FaultAction, ResponseDisposition};
use crate::message::{Message, MessageLimits};
use crate::supervisor::ChannelId;
use crate::traffic::{PacedBody, TrafficDirection, TrafficSchedule};
use crate::transport::{BoxIo, ConnectionContext, PipelinePorts, traffic_schedule};
use crate::{ErrorCode, ProxyError, Result};

type BoxError = Box<dyn Error + Send + Sync>;
type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

/// 在 Hyper 已经完整消费请求 Body 时发出一次通知。
///
/// `DropResponse { read_upstream: false }` 不能等待响应头；同时也不能在请求尚未写完时
/// 提前关闭上游连接。这个薄适配器把“Body 已完整交给 Hyper 写出”变成可等待的边界。
struct CompletionBody<B> {
    inner: B,
    completed: Option<oneshot::Sender<()>>,
    remaining: Option<u64>,
}

impl<B: Body> CompletionBody<B> {
    fn new(inner: B) -> (Self, oneshot::Receiver<()>) {
        let (completed, receiver) = oneshot::channel();
        let remaining = inner.size_hint().exact();
        (
            Self {
                inner,
                completed: Some(completed),
                remaining,
            },
            receiver,
        )
    }
}

impl<B> Body for CompletionBody<B>
where
    B: Body + Unpin,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        let result = Pin::new(&mut self.inner).poll_frame(context);
        if let Poll::Ready(Some(Ok(frame))) = &result
            && let (Some(remaining), Some(data)) = (self.remaining.as_mut(), frame.data_ref())
        {
            *remaining = remaining.saturating_sub(data.remaining() as u64);
        }
        let fully_consumed = matches!(result, Poll::Ready(None))
            || self.remaining.is_some_and(|remaining| remaining == 0);
        if fully_consumed && let Some(completed) = self.completed.take() {
            let _ = completed.send(());
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.completed.is_none() && self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

const COPY_BUFFER_BYTES: usize = 16 * 1024;
const MAX_CLIENT_HELLO_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardAuthenticationMode {
    None,
    Required,
}

#[derive(Debug, Clone)]
pub struct ForwardProxyConfig {
    pub bind_addr: SocketAddr,
    pub authentication: ForwardAuthenticationMode,
    pub allowed_client_cidrs: Vec<String>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    /// 每次成功读取或写入都会重新开始计时，因此它是真正的空闲超时，不是隧道总寿命。
    pub tunnel_idle_timeout: Duration,
}

impl ForwardProxyConfig {
    /// 在打开监听套接字前执行安全校验。
    ///
    /// 回环地址可以无认证用于本机调试；任何非回环地址（包括 `0.0.0.0`/`::`）必须同时
    /// 配置认证和 CIDR 白名单，避免无意创建公网开放代理。
    pub fn validate(&self) -> Result<()> {
        if self.bind_addr.port() == 0 {
            return Err(config_error(
                "forward proxy listen port must be greater than zero",
            ));
        }
        if self.connect_timeout.is_zero()
            || self.read_timeout.is_zero()
            || self.write_timeout.is_zero()
            || self.tunnel_idle_timeout.is_zero()
        {
            return Err(config_error(
                "forward proxy timeouts must be greater than zero",
            ));
        }
        if !self.bind_addr.ip().is_loopback()
            && (self.authentication != ForwardAuthenticationMode::Required
                || self.allowed_client_cidrs.is_empty())
        {
            return Err(config_error(
                "non-loopback forward proxy listeners require authentication and a client CIDR allowlist",
            ));
        }
        for cidr in &self.allowed_client_cidrs {
            Network::parse(cidr).ok_or_else(|| {
                config_error(format!("invalid forward proxy client CIDR {cidr:?}"))
            })?;
        }
        Ok(())
    }

    fn permits_peer(&self, peer: IpAddr) -> bool {
        self.allowed_client_cidrs.is_empty()
            || self
                .allowed_client_cidrs
                .iter()
                .filter_map(|value| Network::parse(value))
                .any(|network| network.contains(peer))
    }
}

/// 代理认证适配边界。实现可以查询系统密钥库，但不得在日志中输出凭据。
pub trait ForwardProxyAuthenticator: Debug + Send + Sync {
    fn authorize(&self, peer: SocketAddr, presented: Option<&HeaderValue>) -> bool;
}

/// 安装级 MITM Root CA 的最小签发边界。
///
/// 实现必须从受系统密钥保护的安装级 Root 读取签名能力。运行时只接收本次 authority 的
/// 叶子证书和临时私钥，不知道数据库、Keychain/DPAPI 或文件布局。
pub trait MitmCertificateAuthority: Debug + Send + Sync {
    fn issue_server_identity(&self, authority_host: &str) -> Result<MitmServerIdentity>;
}

/// 动态叶子证书材料。私钥只进入 rustls `CertifiedKey`，不会被序列化或写盘。
pub struct MitmServerIdentity {
    pub certificate_chain_der: Vec<Vec<u8>>,
    pub private_key_pkcs8_der: zeroize::Zeroizing<Vec<u8>>,
}

impl Debug for MitmServerIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MitmServerIdentity")
            .field("certificate_chain_len", &self.certificate_chain_der.len())
            .field("private_key_pkcs8_der", &"<redacted>")
            .finish()
    }
}

/// MITM 上游 TLS 连接边界。生产实现使用操作系统信任库；测试可注入局部测试 Root。
#[async_trait::async_trait]
pub trait MitmUpstreamConnector: Debug + Send + Sync {
    async fn connect(
        &self,
        authority_host: &str,
        upstream: TcpStream,
        cancellation: &CancellationToken,
    ) -> Result<BoxIo>;
}

#[derive(Debug, Clone)]
pub struct NativeRootMitmConnector {
    config: Arc<ClientConfig>,
}

impl NativeRootMitmConnector {
    pub fn new() -> Result<Self> {
        let mut roots = RootCertStore::empty();
        let loaded = rustls_native_certs::load_native_certs();
        let (added, ignored) = roots.add_parsable_certificates(loaded.certs);
        if added == 0 {
            return Err(ProxyError::new(
                ErrorCode::CertificateNotReady,
                format!(
                    "platform trust store contains no usable certificates ({} load errors, {ignored} invalid certificates)",
                    loaded.errors.len()
                ),
            ));
        }
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(tls_config_error)?
                .with_root_certificates(roots)
                .with_no_client_auth();
        Ok(Self {
            config: Arc::new(config),
        })
    }
}

#[async_trait::async_trait]
impl MitmUpstreamConnector for NativeRootMitmConnector {
    async fn connect(
        &self,
        authority_host: &str,
        upstream: TcpStream,
        cancellation: &CancellationToken,
    ) -> Result<BoxIo> {
        let server_name = ServerName::try_from(authority_host.to_owned())
            .map_err(|error| config_error(format!("invalid MITM upstream server name: {error}")))?;
        let stream = timeout_or_cancel(
            Duration::from_secs(30),
            cancellation,
            TlsConnector::from(self.config.clone()).connect(server_name, upstream),
            ErrorCode::UpstreamConnectTimeout,
        )
        .await?
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::TlsHandshakeFailed,
                format!("MITM upstream TLS handshake failed: {error}"),
            )
        })?;
        Ok(Box::new(stream))
    }
}

#[derive(Debug, Clone)]
pub struct ForwardMitmConfig {
    pub authority_allowlist: Vec<String>,
    pub maximum_cached_leaf_certificates: usize,
}

impl ForwardMitmConfig {
    fn validate(&self) -> Result<()> {
        if self.authority_allowlist.is_empty() {
            return Err(config_error("MITM authority allowlist must not be empty"));
        }
        if !(1..=256).contains(&self.maximum_cached_leaf_certificates) {
            return Err(config_error(
                "MITM leaf certificate cache capacity must be in 1..=256",
            ));
        }
        if self
            .authority_allowlist
            .iter()
            .any(|pattern| !valid_authority_pattern(pattern))
        {
            return Err(config_error(
                "MITM allowlist entries must be exact hosts/IPs or *.example.test patterns",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct MitmLeafCache {
    entries: HashMap<String, Arc<ServerConfig>>,
    recency: VecDeque<String>,
    capacity: usize,
}

impl MitmLeafCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            recency: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn get(&mut self, host: &str) -> Option<Arc<ServerConfig>> {
        let value = self.entries.get(host).cloned()?;
        self.touch(host);
        Some(value)
    }

    fn insert(&mut self, host: &str, value: &Arc<ServerConfig>) {
        self.entries.insert(host.to_owned(), value.clone());
        self.touch(host);
        while self.entries.len() > self.capacity {
            if let Some(evicted) = self.recency.pop_front() {
                self.entries.remove(&evicted);
            }
        }
    }

    fn touch(&mut self, host: &str) {
        self.recency.retain(|entry| entry != host);
        self.recency.push_back(host.to_owned());
    }
}

#[derive(Debug)]
struct ForwardMitmRuntime {
    config: ForwardMitmConfig,
    certificate_authority: Arc<dyn MitmCertificateAuthority>,
    upstream_connector: Arc<dyn MitmUpstreamConnector>,
    leaf_cache: Mutex<MitmLeafCache>,
}

#[derive(Debug)]
struct ForwardPipelineRuntime {
    channel: ChannelId,
    runtime_epoch: Uuid,
    ports: Arc<dyn PipelinePorts>,
    limits: MessageLimits,
}

#[derive(Debug, Default)]
pub struct NoAuthentication;

impl ForwardProxyAuthenticator for NoAuthentication {
    fn authorize(&self, _peer: SocketAddr, _presented: Option<&HeaderValue>) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
pub struct ForwardProxyService {
    config: ForwardProxyConfig,
    authenticator: Arc<dyn ForwardProxyAuthenticator>,
    mitm: Option<Arc<ForwardMitmRuntime>>,
    pipeline: Option<Arc<ForwardPipelineRuntime>>,
}

impl ForwardProxyService {
    pub fn new(
        config: ForwardProxyConfig,
        authenticator: Arc<dyn ForwardProxyAuthenticator>,
    ) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            authenticator,
            mitm: None,
            pipeline: None,
        })
    }

    /// 将可解析的正向 HTTP/1.1 消息接入与 Reverse Listener 相同的应用管线。
    ///
    /// CONNECT tunnel 本身不作为 HTTP 业务报文进入管线；只有 absolute-form HTTP 和
    /// allowlist 命中的 MITM 内层 HTTP/1.1 请求进入。未命中 allowlist 的 TLS/h2/h3
    /// 字节流继续透明转发。
    #[must_use]
    pub fn with_pipeline(
        mut self,
        channel: ChannelId,
        runtime_epoch: Uuid,
        ports: Arc<dyn PipelinePorts>,
        limits: MessageLimits,
    ) -> Self {
        self.pipeline = Some(Arc::new(ForwardPipelineRuntime {
            channel,
            runtime_epoch,
            ports,
            limits,
        }));
        self
    }

    /// 为显式 authority 允许列表启用 HTTPS MITM。
    ///
    /// 未命中允许列表的 CONNECT 仍严格走原始透明 tunnel。该方法不改变默认构造行为，
    /// 因此未提供安装级 CA 时不可能意外拦截 HTTPS。
    pub fn with_mitm(
        mut self,
        config: ForwardMitmConfig,
        certificate_authority: Arc<dyn MitmCertificateAuthority>,
        upstream_connector: Arc<dyn MitmUpstreamConnector>,
    ) -> Result<Self> {
        config.validate()?;
        let capacity = config.maximum_cached_leaf_certificates;
        self.mitm = Some(Arc::new(ForwardMitmRuntime {
            config,
            certificate_authority,
            upstream_connector,
            leaf_cache: Mutex::new(MitmLeafCache::new(capacity)),
        }));
        Ok(self)
    }

    /// 在一条已接受的下游 TCP 连接上提供 HTTP/1.1 正向代理服务。
    pub async fn serve_connection(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let context = self.pipeline.as_ref().map(|pipeline| ConnectionContext {
            runtime_epoch: pipeline.runtime_epoch,
            connection_id: Uuid::new_v4(),
            channel: pipeline.channel.clone(),
            peer_addr: peer,
            accepted_at: SystemTime::now(),
            tls_peer: None,
        });
        if let (Some(pipeline), Some(context)) = (&self.pipeline, &context) {
            pipeline.ports.connection_opened(context).await;
        }
        let service = self.clone();
        let handler_context = context.clone();
        let handler_cancellation = cancellation.clone();
        let handler = service_fn(move |request| {
            let service = service.clone();
            let context = handler_context.clone();
            let cancellation = handler_cancellation.clone();
            async move { service.handle(request, peer, context, cancellation).await }
        });
        let connection = server_http1::Builder::new()
            .keep_alive(true)
            .serve_connection(TokioIo::new(io), handler)
            .with_upgrades();
        let result = tokio::select! {
            () = cancellation.cancelled() => Err(ProxyError::new(
                ErrorCode::ProxyStopped,
                "forward proxy stopped while a client connection was active",
            )),
            result = connection => result.map_err(|error| {
                ProxyError::new(
                    ErrorCode::Io,
                    format!("forward proxy HTTP/1 connection failed: {error}"),
                )
            }),
        };
        if let (Some(pipeline), Some(context)) = (&self.pipeline, &context) {
            pipeline.ports.connection_closed(context, &result).await;
        }
        result
    }

    /// 绑定配置中的地址并运行监听循环。
    ///
    /// 需要统一管理多监听器的 Host 可先自行绑定 `TcpListener`，再调用
    /// [`Self::serve_listener`]；这样能在启动 epoch 发布前完成“全部端口先绑定”的事务式
    /// 准备。单监听器 CLI/测试则可直接使用本方法。
    pub async fn bind_and_serve(&self, cancellation: CancellationToken) -> Result<()> {
        let listener = TcpListener::bind(self.config.bind_addr)
            .await
            .map_err(|error| {
                let code = if error.kind() == std::io::ErrorKind::AddrInUse {
                    ErrorCode::PortInUse
                } else {
                    ErrorCode::Io
                };
                ProxyError::new(
                    code,
                    format!(
                        "cannot bind forward proxy listener {}: {error}",
                        self.config.bind_addr
                    ),
                )
            })?;
        self.serve_listener(listener, cancellation).await
    }

    /// 在已经绑定的 listener 上运行，直到取消或 accept 失败。
    pub async fn serve_listener(
        &self,
        listener: TcpListener,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => break,
                completed = connections.join_next(), if !connections.is_empty() => {
                    if let Some(Err(error)) = completed
                        && !error.is_cancelled()
                    {
                        tracing::warn!(?error, "forward proxy connection task panicked");
                    }
                }
                accepted = listener.accept() => {
                    let (stream, peer) = accepted
                        .map_err(|error| ProxyError::io("accept forward proxy client", &error))?;
                    stream
                        .set_nodelay(true)
                        .map_err(|error| ProxyError::io("configure forward proxy client", &error))?;
                    let service = self.clone();
                    let connection_cancellation = cancellation.clone();
                    connections.spawn(async move {
                        let result = service
                            .serve_connection(Box::new(stream), peer, connection_cancellation)
                            .await;
                        if let Err(error) = &result
                            && error.code != ErrorCode::ProxyStopped.as_str()
                        {
                            tracing::debug!(
                                code = error.code,
                                message = %error.message,
                                %peer,
                                "forward proxy client connection ended"
                            );
                        }
                        result
                    });
                }
            }
        }

        // 所有子任务共享同一取消令牌。正常情况下会立即结束；超出短暂宽限期时强制
        // abort，保证监听器 stop 不会被静默客户端无限拖住。
        let graceful = async { while connections.join_next().await.is_some() {} };
        if tokio::time::timeout(Duration::from_secs(5), graceful)
            .await
            .is_err()
        {
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        }
        Ok(())
    }

    async fn handle(
        &self,
        mut request: Request<Incoming>,
        peer: SocketAddr,
        context: Option<ConnectionContext>,
        cancellation: CancellationToken,
    ) -> Result<Response<ProxyBody>> {
        if !self.config.permits_peer(peer.ip()) {
            return Ok(text_response(
                StatusCode::FORBIDDEN,
                "client address is not allowed",
            ));
        }
        if self.config.authentication == ForwardAuthenticationMode::Required
            && !self
                .authenticator
                .authorize(peer, request.headers().get(PROXY_AUTHORIZATION))
        {
            let mut response = text_response(
                StatusCode::PROXY_AUTHENTICATION_REQUIRED,
                "proxy authentication required",
            );
            response.headers_mut().insert(
                PROXY_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"Intercept Proxy\""),
            );
            return Ok(response);
        }

        if request.method() == Method::CONNECT {
            return Ok(self
                .handle_connect(&mut request, context, cancellation)
                .await
                .unwrap_or_else(|error| error_response(&error)));
        }
        match self
            .forward_http(request, context.as_ref(), cancellation)
            .await
        {
            Ok(response) => Ok(response),
            Err(error) if intentional_response_drop(&error) => Err(error),
            Err(error) => Ok(error_response(&error)),
        }
    }

    async fn handle_connect(
        &self,
        request: &mut Request<Incoming>,
        context: Option<ConnectionContext>,
        cancellation: CancellationToken,
    ) -> Result<Response<ProxyBody>> {
        let authority = connect_authority(request.uri())?;
        let authority_host = authority_host(&authority)?;
        if let Some(mitm) = &self.mitm
            && authority_is_allowed(&authority_host, &mitm.config.authority_allowlist)
        {
            return self
                .handle_mitm_connect(
                    request,
                    authority,
                    authority_host,
                    context,
                    cancellation,
                    mitm.clone(),
                )
                .await;
        }
        let upstream =
            connect_target(&authority, self.config.connect_timeout, &cancellation).await?;
        let upgraded = hyper::upgrade::on(request);
        let idle_timeout = self.config.tunnel_idle_timeout;
        tokio::spawn(async move {
            let result = async {
                let upgraded = upgraded.await.map_err(|error| {
                    ProxyError::new(ErrorCode::Io, format!("CONNECT upgrade failed: {error}"))
                })?;
                run_tunnel(TokioIo::new(upgraded), upstream, idle_timeout, cancellation).await
            }
            .await;
            if let Err(error) = result {
                tracing::debug!(code = error.code, message = %error.message, "CONNECT tunnel ended");
            }
        });
        Ok(empty_response(StatusCode::OK))
    }

    async fn handle_mitm_connect(
        &self,
        request: &mut Request<Incoming>,
        authority: String,
        authority_host: String,
        context: Option<ConnectionContext>,
        cancellation: CancellationToken,
        mitm: Arc<ForwardMitmRuntime>,
    ) -> Result<Response<ProxyBody>> {
        // 在发送 200 前完成签发和上游 TCP 连接。这样 CA/配置错误能作为代理错误返回，而
        // 不是先承诺 tunnel 成功后再静默断开。
        let server_config = mitm.server_config_for(&authority_host).await?;
        let upstream =
            connect_target(&authority, self.config.connect_timeout, &cancellation).await?;
        let upgraded = hyper::upgrade::on(request);
        let read_timeout = self.config.read_timeout;
        let write_timeout = self.config.write_timeout;
        let idle_timeout = self.config.tunnel_idle_timeout;
        let pipeline = self.pipeline.clone();
        tokio::spawn(async move {
            let result = async {
                let upgraded = upgraded.await.map_err(|error| {
                    ProxyError::new(
                        ErrorCode::Io,
                        format!("MITM CONNECT upgrade failed: {error}"),
                    )
                })?;
                let mut downstream = TokioIo::new(upgraded);
                let client_hello =
                    read_client_hello_prefix(&mut downstream, read_timeout, &cancellation).await?;
                if client_hello_requires_tunnel(&client_hello) {
                    let mut upstream = upstream;
                    timeout_or_cancel(
                        write_timeout,
                        &cancellation,
                        upstream.write_all(&client_hello),
                        ErrorCode::UpstreamWriteTimeout,
                    )
                    .await?
                    .map_err(|error| ProxyError::io("forward h2/h3 ClientHello", &error))?;
                    return run_tunnel(downstream, upstream, idle_timeout, cancellation).await;
                }
                let downstream = TlsAcceptor::from(server_config)
                    .accept(PrefixIo::new(client_hello, downstream))
                    .await
                    .map_err(|error| {
                        ProxyError::new(
                            ErrorCode::TlsHandshakeFailed,
                            format!("MITM downstream TLS handshake failed: {error}"),
                        )
                    })?;
                let upstream = mitm
                    .upstream_connector
                    .connect(&authority_host, upstream, &cancellation)
                    .await?;
                serve_mitm_http1(
                    downstream,
                    upstream,
                    authority,
                    read_timeout,
                    write_timeout,
                    idle_timeout,
                    pipeline,
                    context,
                    cancellation,
                )
                .await
            }
            .await;
            if let Err(error) = result {
                tracing::debug!(
                    code = error.code,
                    message = %error.message,
                    "MITM CONNECT session ended"
                );
            }
        });
        Ok(empty_response(StatusCode::OK))
    }

    async fn forward_http(
        &self,
        request: Request<Incoming>,
        context: Option<&ConnectionContext>,
        cancellation: CancellationToken,
    ) -> Result<Response<ProxyBody>> {
        if is_websocket_upgrade(&request) {
            return self.forward_websocket(request, context, cancellation).await;
        }
        let (mut parts, body) = request.into_parts();
        let captured_uri = parts.uri.clone();
        let target = absolute_http_target(&parts.uri)?;
        parts.uri = absolute_uri_to_origin_form(&parts.uri)?;
        strip_hop_by_hop_headers(&mut parts.headers);
        if !parts.headers.contains_key(HOST) {
            parts.headers.insert(
                HOST,
                HeaderValue::from_str(&target.host_header).map_err(|error| {
                    config_error(format!("invalid target Host header: {error}"))
                })?,
            );
        }

        if let (Some(pipeline), Some(context)) = (&self.pipeline, context) {
            return self
                .forward_http_through_pipeline(
                    parts,
                    body,
                    captured_uri,
                    target,
                    pipeline,
                    context,
                    &cancellation,
                )
                .await;
        }

        let stream = connect_target(
            &target.connect_authority,
            self.config.connect_timeout,
            &cancellation,
        )
        .await?;
        let (mut sender, connection) = client_http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| {
                ProxyError::new(
                    ErrorCode::Io,
                    format!("origin HTTP handshake failed: {error}"),
                )
            })?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(?error, "forward origin HTTP connection ended");
            }
        });

        let outgoing = Request::from_parts(parts, incoming_body(body));
        let response = timeout_or_cancel(
            self.config
                .write_timeout
                .saturating_add(self.config.read_timeout),
            &cancellation,
            sender.send_request(outgoing),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::Io,
                format!("origin HTTP request failed: {error}"),
            )
        })?;
        let (mut parts, body) = response.into_parts();
        strip_hop_by_hop_headers(&mut parts.headers);
        Ok(Response::from_parts(parts, incoming_body(body)))
    }

    /// WebSocket 只把 HTTP Upgrade 握手交给通用管线；101 之后的帧保持字节流透明转发。
    async fn forward_websocket(
        &self,
        mut request: Request<Incoming>,
        context: Option<&ConnectionContext>,
        cancellation: CancellationToken,
    ) -> Result<Response<ProxyBody>> {
        let downstream_upgrade = hyper::upgrade::on(&mut request);
        let (mut parts, body) = request.into_parts();
        let captured_uri = parts.uri.clone();
        let target = absolute_http_target(&parts.uri)?;
        parts.uri = absolute_uri_to_origin_form(&parts.uri)?;
        strip_hop_by_hop_headers_preserving_upgrade(&mut parts.headers);
        if !parts.headers.contains_key(HOST) {
            parts.headers.insert(
                HOST,
                HeaderValue::from_str(&target.host_header)
                    .map_err(|error| config_error(format!("invalid Host header: {error}")))?,
            );
        }

        let mut request_actions = Vec::new();
        let outgoing_body = if let (Some(pipeline), Some(context)) = (&self.pipeline, context) {
            let body = collect_pipeline_body(
                body,
                pipeline.limits,
                &cancellation,
                self.config.read_timeout,
            )
            .await?;
            let (message, actions) = prepare_pipeline_request(
                pipeline,
                context,
                &parts.method,
                &captured_uri,
                &parts.headers,
                body,
                &cancellation,
            )
            .await?;
            if let Some(response) = request_terminal_response(&actions, &cancellation)? {
                return Ok(response);
            }
            reject_websocket_drop(&actions)?;
            parts.headers = message.header_map()?;
            ensure_websocket_upgrade_headers(&mut parts.headers);
            request_actions = actions;
            message.body
        } else {
            body.collect()
                .await
                .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?
                .to_bytes()
        };
        parts.headers.remove(http::header::CONTENT_LENGTH);
        if !outgoing_body.is_empty() {
            parts.headers.insert(
                http::header::CONTENT_LENGTH,
                HeaderValue::from_str(&outgoing_body.len().to_string())
                    .map_err(|error| config_error(format!("invalid content length: {error}")))?,
            );
        }

        let stream = connect_target(
            &target.connect_authority,
            self.config.connect_timeout,
            &cancellation,
        )
        .await?;
        let (mut sender, connection) = client_http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        tokio::spawn(async move {
            if let Err(error) = connection.with_upgrades().await {
                tracing::debug!(?error, "WebSocket origin connection ended");
            }
        });
        let schedule = traffic_schedule(&request_actions, TrafficDirection::Upstream)?;
        let outgoing = Request::from_parts(
            parts,
            scheduled_body(
                outgoing_body.clone(),
                outgoing_body.len(),
                schedule,
                &cancellation,
            ),
        );
        let mut response = timeout_or_cancel(
            self.config
                .write_timeout
                .saturating_add(self.config.read_timeout),
            &cancellation,
            sender.send_request(outgoing),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        if response.status() != StatusCode::SWITCHING_PROTOCOLS {
            if let (Some(pipeline), Some(context)) = (&self.pipeline, context) {
                return finish_pipeline_response(
                    pipeline,
                    context,
                    response,
                    &cancellation,
                    self.config.read_timeout,
                )
                .await;
            }
            let (mut parts, body) = response.into_parts();
            strip_hop_by_hop_headers(&mut parts.headers);
            return Ok(Response::from_parts(parts, incoming_body(body)));
        }

        let upstream_upgrade = hyper::upgrade::on(&mut response);
        let (mut parts, _body) = response.into_parts();
        if let (Some(pipeline), Some(context)) = (&self.pipeline, context) {
            parts = record_websocket_response(pipeline, context, parts, &cancellation).await?;
        }
        ensure_websocket_upgrade_headers(&mut parts.headers);
        let idle_timeout = self.config.tunnel_idle_timeout;
        let tunnel_cancellation = cancellation.clone();
        tokio::spawn(async move {
            let result = async {
                let (downstream, upstream) = tokio::try_join!(downstream_upgrade, upstream_upgrade)
                    .map_err(|error| {
                        ProxyError::new(ErrorCode::Io, format!("WebSocket upgrade failed: {error}"))
                    })?;
                run_tunnel(
                    TokioIo::new(downstream),
                    TokioIo::new(upstream),
                    idle_timeout,
                    tunnel_cancellation,
                )
                .await
            }
            .await;
            if let Err(error) = result {
                tracing::debug!(code = error.code, message = %error.message, "WebSocket tunnel ended");
            }
        });
        Ok(Response::from_parts(parts, full_body(Bytes::new())))
    }

    // 正向代理管线需要同时保留客户端捕获 URI 和已解析的上游目标；拆成更多薄函数只会
    // 隐藏这一协议边界，因此在此处显式保留完整参数集。
    #[allow(clippy::too_many_arguments)]
    async fn forward_http_through_pipeline(
        &self,
        mut parts: http::request::Parts,
        body: Incoming,
        captured_uri: Uri,
        target: HttpTarget,
        pipeline: &ForwardPipelineRuntime,
        context: &ConnectionContext,
        cancellation: &CancellationToken,
    ) -> Result<Response<ProxyBody>> {
        let body = collect_pipeline_body(
            body,
            pipeline.limits,
            cancellation,
            self.config.read_timeout,
        )
        .await?;
        let (message, actions) = prepare_pipeline_request(
            pipeline,
            context,
            &parts.method,
            &captured_uri,
            &parts.headers,
            body,
            cancellation,
        )
        .await?;
        if let Some(response) = request_terminal_response(&actions, cancellation)? {
            return Ok(response);
        }
        parts.headers = message.header_map()?;
        strip_hop_by_hop_headers(&mut parts.headers);
        if !parts.headers.contains_key(HOST) {
            parts.headers.insert(
                HOST,
                HeaderValue::from_str(&target.host_header).map_err(|error| {
                    config_error(format!("invalid target Host header: {error}"))
                })?,
            );
        }
        parts.headers.remove(http::header::CONTENT_LENGTH);
        parts.headers.insert(
            http::header::CONTENT_LENGTH,
            HeaderValue::from_str(&message.body.len().to_string())
                .map_err(|error| config_error(format!("invalid content length: {error}")))?,
        );
        let stream = connect_target(
            &target.connect_authority,
            self.config.connect_timeout,
            cancellation,
        )
        .await?;
        let (mut sender, connection) = client_http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        let upstream_shutdown = CancellationToken::new();
        let connection_shutdown = upstream_shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                () = connection_shutdown.cancelled() => {}
                result = connection => if let Err(error) = result {
                    tracing::debug!(?error, "forward pipeline origin connection ended");
                }
            }
        });
        let schedule = traffic_schedule(&actions, TrafficDirection::Upstream)?;
        let effective_timeout = self
            .config
            .write_timeout
            .saturating_add(self.config.read_timeout)
            .saturating_add(schedule.estimated_delay(message.body.len()));
        let mode = drop_response_mode(&actions);
        let body = scheduled_body(
            message.body.clone(),
            message.body.len(),
            schedule,
            cancellation,
        );
        let (body, body_written) = if mode == Some(DropResponseMode::AfterRequestWrite) {
            let (body, completed) = completion_body(body);
            (body, Some(completed))
        } else {
            (body, None)
        };
        let outgoing = Request::from_parts(parts, body);
        if let Some(body_written) = body_written {
            return send_request_then_drop_after_write(
                &mut sender,
                outgoing,
                body_written,
                &upstream_shutdown,
                cancellation,
                effective_timeout,
                "forward",
            )
            .await
            .map(|_| unreachable!("drop helper only returns errors"));
        }
        let response = timeout_or_cancel(
            effective_timeout,
            cancellation,
            sender.send_request(outgoing),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        if mode == Some(DropResponseMode::AfterUpstreamBody) {
            drain_upstream_body(response.into_body(), cancellation, self.config.read_timeout)
                .await?;
            upstream_shutdown.cancel();
            return Err(intentional_drop_error("forward"));
        }
        finish_pipeline_response(
            pipeline,
            context,
            response,
            cancellation,
            self.config.read_timeout,
        )
        .await
    }
}

impl ForwardMitmRuntime {
    async fn server_config_for(&self, authority_host: &str) -> Result<Arc<ServerConfig>> {
        let cache_key = authority_host.to_ascii_lowercase();
        if let Some(config) = self.leaf_cache.lock().await.get(&cache_key) {
            return Ok(config);
        }

        // 签发可能涉及解密受保护 Root 私钥，不能持有异步缓存锁跨越该边界。允许极少量
        // 同 authority 并发首请求重复签发，最终只缓存一个，避免全局连接队头阻塞。
        let identity = self
            .certificate_authority
            .issue_server_identity(authority_host)?;
        if identity.certificate_chain_der.is_empty() {
            return Err(ProxyError::new(
                ErrorCode::CertificateInvalid,
                "MITM leaf certificate chain is empty",
            ));
        }
        let certificates = identity
            .certificate_chain_der
            .into_iter()
            .map(CertificateDer::from)
            .collect();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            identity.private_key_pkcs8_der.to_vec(),
        ));
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(tls_config_error)?
                .with_no_client_auth()
                .with_single_cert(certificates, key)
                .map_err(tls_config_error)?;
        let config = Arc::new(config);
        let mut cache = self.leaf_cache.lock().await;
        if let Some(existing) = cache.get(&cache_key) {
            return Ok(existing);
        }
        cache.insert(&cache_key, &config);
        Ok(config)
    }
}

// 此函数是单个 MITM 会话的装配边界；超时、管线、连接上下文和取消令牌均属于会话配置。
#[allow(clippy::too_many_arguments)]
async fn serve_mitm_http1<D>(
    downstream: D,
    upstream: BoxIo,
    authority: String,
    read_timeout: Duration,
    write_timeout: Duration,
    idle_timeout: Duration,
    pipeline: Option<Arc<ForwardPipelineRuntime>>,
    context: Option<ConnectionContext>,
    cancellation: CancellationToken,
) -> Result<()>
where
    D: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, upstream_connection) = client_http1::handshake(TokioIo::new(upstream))
        .await
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::Io,
                format!("MITM upstream HTTP/1 handshake failed: {error}"),
            )
        })?;
    let sender = Arc::new(Mutex::new(sender));
    let upstream_shutdown = CancellationToken::new();
    let upstream_cancel = cancellation.clone();
    let upstream_drop = upstream_shutdown.clone();
    let upstream_task = tokio::spawn(async move {
        tokio::select! {
            () = upstream_cancel.cancelled() => Ok(()),
            () = upstream_drop.cancelled() => Ok(()),
            result = upstream_connection.with_upgrades() => result.map_err(|error| ProxyError::new(
                ErrorCode::Io,
                format!("MITM upstream HTTP/1 connection failed: {error}"),
            )),
        }
    });

    let handler_cancellation = cancellation.clone();
    let handler = service_fn(move |request: Request<Incoming>| {
        let sender = sender.clone();
        let authority = authority.clone();
        let pipeline = pipeline.clone();
        let context = context.clone();
        let cancellation = handler_cancellation.clone();
        let upstream_shutdown = upstream_shutdown.clone();
        async move {
            let result = forward_mitm_request(
                request,
                &authority,
                sender,
                read_timeout,
                write_timeout,
                idle_timeout,
                pipeline.as_deref(),
                context.as_ref(),
                &upstream_shutdown,
                &cancellation,
            )
            .await;
            match result {
                Ok(response) => Ok(response),
                Err(error) if intentional_response_drop(&error) => Err(error),
                Err(error) => Ok(error_response(&error)),
            }
        }
    });
    let downstream_connection = server_http1::Builder::new()
        .keep_alive(true)
        .serve_connection(TokioIo::new(downstream), handler)
        .with_upgrades();
    let downstream_result = tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "MITM session cancelled",
        )),
        result = downstream_connection => result.map_err(|error| {
            ProxyError::new(
                ErrorCode::Io,
                format!("MITM downstream HTTP/1 connection failed: {error}"),
            )
        }),
    };
    upstream_task.abort();
    let _ = upstream_task.await;
    downstream_result
}

// HTTP handler 必须显式携带连接复用 sender 以及会话级管线/超时上下文。
#[allow(clippy::too_many_arguments)]
async fn forward_mitm_request(
    request: Request<Incoming>,
    authority: &str,
    sender: Arc<Mutex<client_http1::SendRequest<ProxyBody>>>,
    read_timeout: Duration,
    write_timeout: Duration,
    idle_timeout: Duration,
    pipeline: Option<&ForwardPipelineRuntime>,
    context: Option<&ConnectionContext>,
    upstream_shutdown: &CancellationToken,
    cancellation: &CancellationToken,
) -> Result<Response<ProxyBody>> {
    if is_websocket_upgrade(&request) {
        return forward_mitm_websocket(
            request,
            authority,
            sender,
            read_timeout,
            write_timeout,
            idle_timeout,
            pipeline,
            context,
            cancellation,
        )
        .await;
    }
    let (mut parts, body) = request.into_parts();
    // CONNECT 内部客户端通常发送 origin-form。若它发送 absolute-form，只允许与 CONNECT
    // authority 相同的 https URI，防止一条已授权隧道被借来访问其他主机。
    if parts.uri.scheme().is_some() {
        let uri_authority = parts
            .uri
            .authority()
            .ok_or_else(|| config_error("MITM absolute URI is missing authority"))?
            .as_str()
            .to_owned();
        let normalized = connect_authority(&parts.uri)?;
        if !normalized.eq_ignore_ascii_case(authority) {
            return Err(config_error(
                "MITM request authority differs from CONNECT authority",
            ));
        }
        let origin = parts
            .uri
            .path_and_query()
            .map_or("/", http::uri::PathAndQuery::as_str);
        parts.uri = origin
            .parse()
            .map_err(|error| config_error(format!("invalid MITM origin-form URI: {error}")))?;
        if !parts.headers.contains_key(HOST) {
            parts.headers.insert(
                HOST,
                HeaderValue::from_str(&uri_authority)
                    .map_err(|error| config_error(format!("invalid MITM Host header: {error}")))?,
            );
        }
    }
    strip_hop_by_hop_headers(&mut parts.headers);
    if !parts.headers.contains_key(HOST) {
        parts.headers.insert(
            HOST,
            HeaderValue::from_str(authority)
                .map_err(|error| config_error(format!("invalid MITM Host header: {error}")))?,
        );
    }
    if let (Some(pipeline), Some(context)) = (pipeline, context) {
        let body = collect_pipeline_body(body, pipeline.limits, cancellation, read_timeout).await?;
        let (message, actions) = prepare_pipeline_request(
            pipeline,
            context,
            &parts.method,
            &parts.uri,
            &parts.headers,
            body,
            cancellation,
        )
        .await?;
        if let Some(response) = request_terminal_response(&actions, cancellation)? {
            return Ok(response);
        }
        parts.headers = message.header_map()?;
        strip_hop_by_hop_headers(&mut parts.headers);
        parts.headers.remove(http::header::CONTENT_LENGTH);
        parts.headers.insert(
            http::header::CONTENT_LENGTH,
            HeaderValue::from_str(&message.body.len().to_string())
                .map_err(|error| config_error(format!("invalid content length: {error}")))?,
        );
        let schedule = traffic_schedule(&actions, TrafficDirection::Upstream)?;
        let effective_timeout = write_timeout
            .saturating_add(read_timeout)
            .saturating_add(schedule.estimated_delay(message.body.len()));
        let mode = drop_response_mode(&actions);
        let body = scheduled_body(
            message.body.clone(),
            message.body.len(),
            schedule,
            cancellation,
        );
        let (body, body_written) = if mode == Some(DropResponseMode::AfterRequestWrite) {
            let (body, completed) = completion_body(body);
            (body, Some(completed))
        } else {
            (body, None)
        };
        let outgoing = Request::from_parts(parts, body);
        let mut sender = sender.lock().await;
        if let Some(body_written) = body_written {
            return send_request_then_drop_after_write(
                &mut sender,
                outgoing,
                body_written,
                upstream_shutdown,
                cancellation,
                effective_timeout,
                "MITM",
            )
            .await
            .map(|_| unreachable!("drop helper only returns errors"));
        }
        let response = timeout_or_cancel(
            effective_timeout,
            cancellation,
            sender.send_request(outgoing),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::Io,
                format!("MITM upstream HTTP request failed: {error}"),
            )
        })?;
        if mode == Some(DropResponseMode::AfterUpstreamBody) {
            drain_upstream_body(response.into_body(), cancellation, read_timeout).await?;
            upstream_shutdown.cancel();
            return Err(intentional_drop_error("MITM"));
        }
        return finish_pipeline_response(pipeline, context, response, cancellation, read_timeout)
            .await;
    }
    let outgoing = Request::from_parts(parts, incoming_body(body));
    let mut sender = sender.lock().await;
    let response = timeout_or_cancel(
        write_timeout.saturating_add(read_timeout),
        cancellation,
        sender.send_request(outgoing),
        ErrorCode::UpstreamReadTimeout,
    )
    .await?
    .map_err(|error| {
        ProxyError::new(
            ErrorCode::Io,
            format!("MITM upstream HTTP request failed: {error}"),
        )
    })?;
    let (mut parts, body) = response.into_parts();
    strip_hop_by_hop_headers(&mut parts.headers);
    // Incoming -> Incoming body adapter is streaming and performs no collect/decode/re-encode;
    // therefore an unmodified body is byte-for-byte forwarded with backpressure.
    Ok(Response::from_parts(parts, incoming_body(body)))
}

#[allow(clippy::too_many_arguments)]
async fn forward_mitm_websocket(
    mut request: Request<Incoming>,
    authority: &str,
    sender: Arc<Mutex<client_http1::SendRequest<ProxyBody>>>,
    read_timeout: Duration,
    write_timeout: Duration,
    idle_timeout: Duration,
    pipeline: Option<&ForwardPipelineRuntime>,
    context: Option<&ConnectionContext>,
    cancellation: &CancellationToken,
) -> Result<Response<ProxyBody>> {
    let downstream_upgrade = hyper::upgrade::on(&mut request);
    let (mut parts, body) = request.into_parts();
    if parts.uri.scheme().is_some() {
        let normalized = connect_authority(&parts.uri)?;
        if !normalized.eq_ignore_ascii_case(authority) {
            return Err(config_error(
                "MITM WebSocket authority differs from CONNECT authority",
            ));
        }
        let origin = parts
            .uri
            .path_and_query()
            .map_or("/", http::uri::PathAndQuery::as_str);
        parts.uri = origin
            .parse()
            .map_err(|error| config_error(format!("invalid WebSocket URI: {error}")))?;
    }
    strip_hop_by_hop_headers_preserving_upgrade(&mut parts.headers);
    if !parts.headers.contains_key(HOST) {
        parts.headers.insert(
            HOST,
            HeaderValue::from_str(authority)
                .map_err(|error| config_error(format!("invalid Host header: {error}")))?,
        );
    }

    let mut request_actions = Vec::new();
    let outgoing_body = if let (Some(pipeline), Some(context)) = (pipeline, context) {
        let body = collect_pipeline_body(body, pipeline.limits, cancellation, read_timeout).await?;
        let (message, actions) = prepare_pipeline_request(
            pipeline,
            context,
            &parts.method,
            &parts.uri,
            &parts.headers,
            body,
            cancellation,
        )
        .await?;
        if let Some(response) = request_terminal_response(&actions, cancellation)? {
            return Ok(response);
        }
        reject_websocket_drop(&actions)?;
        parts.headers = message.header_map()?;
        ensure_websocket_upgrade_headers(&mut parts.headers);
        request_actions = actions;
        message.body
    } else {
        body.collect()
            .await
            .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?
            .to_bytes()
    };
    parts.headers.remove(http::header::CONTENT_LENGTH);
    let schedule = traffic_schedule(&request_actions, TrafficDirection::Upstream)?;
    let outgoing = Request::from_parts(
        parts,
        scheduled_body(
            outgoing_body.clone(),
            outgoing_body.len(),
            schedule,
            cancellation,
        ),
    );
    let mut sender = sender.lock().await;
    let mut response = timeout_or_cancel(
        write_timeout.saturating_add(read_timeout),
        cancellation,
        sender.send_request(outgoing),
        ErrorCode::UpstreamReadTimeout,
    )
    .await?
    .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
    drop(sender);
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        if let (Some(pipeline), Some(context)) = (pipeline, context) {
            return finish_pipeline_response(
                pipeline,
                context,
                response,
                cancellation,
                read_timeout,
            )
            .await;
        }
        let (mut parts, body) = response.into_parts();
        strip_hop_by_hop_headers(&mut parts.headers);
        return Ok(Response::from_parts(parts, incoming_body(body)));
    }

    let upstream_upgrade = hyper::upgrade::on(&mut response);
    let (mut parts, _body) = response.into_parts();
    if let (Some(pipeline), Some(context)) = (pipeline, context) {
        parts = record_websocket_response(pipeline, context, parts, cancellation).await?;
    }
    ensure_websocket_upgrade_headers(&mut parts.headers);
    let tunnel_cancellation = cancellation.clone();
    tokio::spawn(async move {
        let result = async {
            let (downstream, upstream) = tokio::try_join!(downstream_upgrade, upstream_upgrade)
                .map_err(|error| {
                    ProxyError::new(
                        ErrorCode::Io,
                        format!("MITM WebSocket upgrade failed: {error}"),
                    )
                })?;
            run_tunnel(
                TokioIo::new(downstream),
                TokioIo::new(upstream),
                idle_timeout,
                tunnel_cancellation,
            )
            .await
        }
        .await;
        if let Err(error) = result {
            tracing::debug!(code = error.code, message = %error.message, "MITM WebSocket tunnel ended");
        }
    });
    Ok(Response::from_parts(parts, full_body(Bytes::new())))
}

async fn collect_pipeline_body(
    body: Incoming,
    limits: MessageLimits,
    cancellation: &CancellationToken,
    read_timeout: Duration,
) -> Result<Bytes> {
    let collected = timeout_or_cancel(
        read_timeout,
        cancellation,
        body.collect(),
        ErrorCode::UpstreamReadTimeout,
    )
    .await?
    .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?
    .to_bytes();
    if collected.len() > limits.max_body_bytes {
        return Err(ProxyError::new(
            ErrorCode::BodyTooLarge,
            format!(
                "forward proxy body is {} bytes; limit is {}",
                collected.len(),
                limits.max_body_bytes
            ),
        ));
    }
    Ok(collected)
}

async fn prepare_pipeline_request(
    pipeline: &ForwardPipelineRuntime,
    context: &ConnectionContext,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: Bytes,
    cancellation: &CancellationToken,
) -> Result<(Message, Vec<FaultAction>)> {
    let mut message = Message::request(method, uri, headers, body);
    message.validate(pipeline.limits)?;
    let actions = pipeline.ports.request(context, &mut message).await?;
    message.validate(pipeline.limits)?;
    for action in &actions {
        match action {
            FaultAction::Delay(duration) => {
                fault::cancellable_delay(*duration, cancellation).await?;
            }
            FaultAction::DisconnectBeforeUpstream => {
                return Err(ProxyError::new(
                    ErrorCode::ClientDisconnected,
                    "forward request intentionally disconnected before upstream",
                ));
            }
            FaultAction::RejectTls => {
                return Err(ProxyError::new(
                    ErrorCode::TlsHandshakeFailed,
                    "forward request intentionally rejected",
                ));
            }
            FaultAction::UpstreamConnectTimeout(duration)
            | FaultAction::UpstreamWriteTimeout(duration)
            | FaultAction::UpstreamReadTimeout(duration) => {
                fault::cancellable_delay(*duration, cancellation).await?;
                return Err(ProxyError::new(
                    match action {
                        FaultAction::UpstreamConnectTimeout(_) => ErrorCode::UpstreamConnectTimeout,
                        FaultAction::UpstreamWriteTimeout(_) => ErrorCode::UpstreamWriteTimeout,
                        _ => ErrorCode::UpstreamReadTimeout,
                    },
                    "forward request injected timeout completed",
                ));
            }
            _ => {}
        }
    }
    Ok((message, actions))
}

fn request_terminal_response(
    actions: &[FaultAction],
    cancellation: &CancellationToken,
) -> Result<Option<Response<ProxyBody>>> {
    for action in actions {
        if let FaultAction::MockResponse {
            status,
            headers,
            body,
        } = action
        {
            let message = fault::mock_response(*status, headers, body.clone());
            return response_from_pipeline_disposition(
                ResponseDisposition::Send {
                    message,
                    schedule: TrafficSchedule::default(),
                },
                cancellation,
            );
        }
    }
    if cancellation.is_cancelled() {
        return Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "forward pipeline cancelled",
        ));
    }
    Ok(None)
}

async fn finish_pipeline_response(
    pipeline: &ForwardPipelineRuntime,
    context: &ConnectionContext,
    response: Response<Incoming>,
    cancellation: &CancellationToken,
    read_timeout: Duration,
) -> Result<Response<ProxyBody>> {
    let (mut parts, body) = response.into_parts();
    strip_hop_by_hop_headers(&mut parts.headers);
    let body = collect_pipeline_body(body, pipeline.limits, cancellation, read_timeout).await?;
    let mut message = Message::response(parts.status, &parts.headers, body);
    message.validate(pipeline.limits)?;
    let actions = pipeline.ports.response(context, &mut message).await?;
    message.validate(pipeline.limits)?;
    let disposition = fault::apply_response_actions(message, &actions, cancellation).await?;
    response_from_pipeline_disposition(disposition, cancellation)?.ok_or_else(|| {
        ProxyError::new(
            ErrorCode::ClientDisconnected,
            "forward response intentionally dropped",
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropResponseMode {
    AfterRequestWrite,
    AfterUpstreamBody,
}

fn drop_response_mode(actions: &[FaultAction]) -> Option<DropResponseMode> {
    actions.iter().find_map(|action| match action {
        FaultAction::DropResponse {
            read_upstream: false,
        } => Some(DropResponseMode::AfterRequestWrite),
        FaultAction::DropResponse {
            read_upstream: true,
        } => Some(DropResponseMode::AfterUpstreamBody),
        _ => None,
    })
}

fn intentional_response_drop(error: &ProxyError) -> bool {
    error.code == ErrorCode::ClientDisconnected.as_str()
        && error.message.contains("response intentionally dropped")
}

fn intentional_drop_error(scope: &str) -> ProxyError {
    ProxyError::new(
        ErrorCode::ClientDisconnected,
        format!("{scope} response intentionally dropped"),
    )
}

fn reject_websocket_drop(actions: &[FaultAction]) -> Result<()> {
    if drop_response_mode(actions).is_some() {
        return Err(config_error(
            "DropResponse is not supported for WebSocket Upgrade; use a connection fault after the 101 handshake",
        ));
    }
    Ok(())
}

fn completion_body(body: ProxyBody) -> (ProxyBody, oneshot::Receiver<()>) {
    let (body, completed) = CompletionBody::new(body);
    (body.boxed_unsync(), completed)
}

async fn drain_upstream_body(
    mut body: Incoming,
    cancellation: &CancellationToken,
    read_timeout: Duration,
) -> Result<()> {
    timeout_or_cancel(
        read_timeout,
        cancellation,
        async {
            while let Some(frame) = body.frame().await {
                frame.map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
            }
            Ok(())
        },
        ErrorCode::UpstreamReadTimeout,
    )
    .await??;
    Ok(())
}

async fn send_request_then_drop_after_write(
    sender: &mut client_http1::SendRequest<ProxyBody>,
    request: Request<ProxyBody>,
    mut body_written: oneshot::Receiver<()>,
    upstream_shutdown: &CancellationToken,
    cancellation: &CancellationToken,
    timeout: Duration,
    scope: &str,
) -> Result<Response<Incoming>> {
    let send = sender.send_request(request);
    tokio::pin!(send);
    tokio::select! {
        written = &mut body_written => {
            written.map_err(|_| ProxyError::new(
                ErrorCode::Io,
                format!("{scope} request body ended before the complete-write boundary"),
            ))?;
        }
        response = timeout_or_cancel(
            timeout,
            cancellation,
            &mut send,
            ErrorCode::UpstreamReadTimeout,
        ) => {
            response?
                .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
            timeout_or_cancel(
                timeout,
                cancellation,
                &mut body_written,
                ErrorCode::UpstreamWriteTimeout,
            )
            .await?
            .map_err(|_| ProxyError::new(
                ErrorCode::Io,
                format!("{scope} request body ended before the complete-write boundary"),
            ))?;
        }
    }
    // `Body` 的最后一帧已交给 Hyper；让连接任务完成当前一次 socket 写入，再触发关闭。
    // 这不会等待响应头，且测试用延迟响应头上游会验证该边界。
    tokio::task::yield_now().await;
    upstream_shutdown.cancel();
    Err(intentional_drop_error(scope))
}

fn response_from_pipeline_disposition(
    disposition: ResponseDisposition,
    cancellation: &CancellationToken,
) -> Result<Option<Response<ProxyBody>>> {
    let (message, body, schedule) = match disposition {
        ResponseDisposition::Send { message, schedule } => {
            let body = message.body.clone();
            (message, body, schedule)
        }
        ResponseDisposition::Drop => return Ok(None),
        ResponseDisposition::Truncate {
            message,
            bytes,
            schedule,
        } => {
            let body = message.body.slice(..bytes);
            (message, body, schedule)
        }
    };
    let status = message.http_status().ok_or_else(|| {
        ProxyError::new(
            ErrorCode::Internal,
            "pipeline response has an invalid HTTP status line",
        )
    })?;
    let mut headers = message.header_map()?;
    strip_hop_by_hop_headers(&mut headers);
    let claimed_length = message.declared_content_length().unwrap_or(body.len());
    let mut response = Response::builder()
        .status(status)
        .body(scheduled_body(body, claimed_length, schedule, cancellation))
        .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
    *response.headers_mut() = headers;
    Ok(Some(response))
}

#[derive(Debug)]
struct HttpTarget {
    connect_authority: String,
    host_header: String,
}

fn absolute_http_target(uri: &Uri) -> Result<HttpTarget> {
    if uri.scheme_str() != Some("http") {
        return Err(config_error(
            "non-CONNECT forward proxy requests require an absolute http URI",
        ));
    }
    let authority = uri
        .authority()
        .ok_or_else(|| config_error("absolute request URI is missing authority"))?;
    if authority.as_str().contains('@') {
        return Err(config_error(
            "forward proxy target must not contain userinfo",
        ));
    }
    let host = uri
        .host()
        .ok_or_else(|| config_error("target host is missing"))?;
    let port = uri.port_u16().unwrap_or(80);
    if port == 0 {
        return Err(config_error("target port must be greater than zero"));
    }
    let host = unbracket_host(host);
    let connect_authority = format_authority(host, port);
    let host_header = if port == 80 {
        format_host(host)
    } else {
        connect_authority.clone()
    };
    Ok(HttpTarget {
        connect_authority,
        host_header,
    })
}

/// 将正向代理 absolute-form request-target 转为上游需要的 origin-form。
pub fn absolute_uri_to_origin_form(uri: &Uri) -> Result<Uri> {
    if uri.scheme_str() != Some("http") || uri.authority().is_none() {
        return Err(config_error("request-target is not an absolute HTTP URI"));
    }
    let origin = uri
        .path_and_query()
        .map_or("/", http::uri::PathAndQuery::as_str);
    origin
        .parse()
        .map_err(|error| config_error(format!("invalid origin-form request-target: {error}")))
}

/// 删除 RFC 9110 hop-by-hop 字段及 `Connection` 动态声明的字段。
pub fn strip_hop_by_hop_headers(headers: &mut HeaderMap) {
    let connection_tokens = headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|value| HeaderName::from_bytes(value.trim().as_bytes()).ok())
        .collect::<Vec<_>>();
    for name in connection_tokens {
        headers.remove(name);
    }
    for name in [
        CONNECTION,
        HeaderName::from_static("proxy-connection"),
        HeaderName::from_static("keep-alive"),
        PROXY_AUTHENTICATE,
        PROXY_AUTHORIZATION,
        TE,
        TRAILER,
        TRANSFER_ENCODING,
        UPGRADE,
    ] {
        headers.remove(name);
    }
}

fn is_websocket_upgrade(request: &Request<Incoming>) -> bool {
    request
        .headers()
        .get(UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && request
            .headers()
            .get_all(CONNECTION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
}

fn strip_hop_by_hop_headers_preserving_upgrade(headers: &mut HeaderMap) {
    let connection_tokens = headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter(|value| !value.trim().eq_ignore_ascii_case("upgrade"))
        .filter_map(|value| HeaderName::from_bytes(value.trim().as_bytes()).ok())
        .collect::<Vec<_>>();
    for name in connection_tokens {
        headers.remove(name);
    }
    for name in [
        HeaderName::from_static("proxy-connection"),
        HeaderName::from_static("keep-alive"),
        PROXY_AUTHENTICATE,
        PROXY_AUTHORIZATION,
        TE,
        TRAILER,
        TRANSFER_ENCODING,
    ] {
        headers.remove(name);
    }
    ensure_websocket_upgrade_headers(headers);
}

fn ensure_websocket_upgrade_headers(headers: &mut HeaderMap) {
    headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
    headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
}

async fn record_websocket_response(
    pipeline: &ForwardPipelineRuntime,
    context: &ConnectionContext,
    parts: http::response::Parts,
    cancellation: &CancellationToken,
) -> Result<http::response::Parts> {
    let mut message = Message::response(parts.status, &parts.headers, Bytes::new());
    message.validate(pipeline.limits)?;
    let actions = pipeline.ports.response(context, &mut message).await?;
    message.validate(pipeline.limits)?;
    let disposition = fault::apply_response_actions(message, &actions, cancellation).await?;
    let response =
        response_from_pipeline_disposition(disposition, cancellation)?.ok_or_else(|| {
            ProxyError::new(
                ErrorCode::ClientDisconnected,
                "WebSocket handshake response intentionally dropped",
            )
        })?;
    let (mut parts, _body) = response.into_parts();
    if parts.status == StatusCode::SWITCHING_PROTOCOLS {
        ensure_websocket_upgrade_headers(&mut parts.headers);
    }
    Ok(parts)
}

fn connect_authority(uri: &Uri) -> Result<String> {
    let authority = uri
        .authority()
        .map_or_else(|| uri.path(), http::uri::Authority::as_str);
    if authority.is_empty() || authority.contains('@') || authority.contains('/') {
        return Err(config_error(
            "CONNECT requires a valid authority-form target",
        ));
    }
    let parsed = authority
        .parse::<http::uri::Authority>()
        .map_err(|error| config_error(format!("invalid CONNECT authority: {error}")))?;
    let host = unbracket_host(parsed.host());
    let port = parsed.port_u16().unwrap_or(443);
    if port == 0 {
        return Err(config_error(
            "CONNECT target port must be greater than zero",
        ));
    }
    Ok(format_authority(host, port))
}

fn authority_host(authority: &str) -> Result<String> {
    authority
        .parse::<http::uri::Authority>()
        .map(|parsed| unbracket_host(parsed.host()).to_ascii_lowercase())
        .map_err(|error| config_error(format!("invalid authority host: {error}")))
}

/// 精确主机/IP 或 `*.example.test` 后缀匹配。通配符不匹配裸根域，且边界必须是 `.`，
/// 因而 `badexample.test` 不会误命中 `*.example.test`。
pub(crate) fn authority_is_allowed(host: &str, patterns: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
        if let Some(suffix) = pattern.strip_prefix("*.") {
            host.len() > suffix.len()
                && host.ends_with(suffix)
                && host.as_bytes().get(host.len() - suffix.len() - 1) == Some(&b'.')
        } else {
            host == pattern
        }
    })
}

fn valid_authority_pattern(pattern: &str) -> bool {
    let value = pattern.trim();
    if value.is_empty() || value.contains(['/', ':', '@']) {
        return value.parse::<IpAddr>().is_ok();
    }
    let host = value.strip_prefix("*.").unwrap_or(value);
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

async fn connect_target(
    authority: &str,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<TcpStream> {
    let stream = timeout_or_cancel(
        timeout,
        cancellation,
        TcpStream::connect(authority),
        ErrorCode::UpstreamConnectTimeout,
    )
    .await?
    .map_err(|error| ProxyError::io("connect forward proxy target", &error))?;
    stream
        .set_nodelay(true)
        .map_err(|error| ProxyError::io("configure forward proxy target", &error))?;
    Ok(stream)
}

#[derive(Debug)]
struct PrefixIo<T> {
    prefix: Bytes,
    offset: usize,
    inner: T,
}

impl<T> PrefixIo<T> {
    fn new(prefix: Bytes, inner: T) -> Self {
        Self {
            prefix,
            offset: 0,
            inner,
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for PrefixIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.offset < self.prefix.len() && buffer.remaining() > 0 {
            let available = &self.prefix[self.offset..];
            let count = available.len().min(buffer.remaining());
            buffer.put_slice(&available[..count]);
            self.offset += count;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for PrefixIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

async fn read_client_hello_prefix<T: AsyncRead + Unpin>(
    io: &mut T,
    read_timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Bytes> {
    let mut records = Vec::new();
    let mut handshake = Vec::new();
    loop {
        let mut header = [0u8; 5];
        timeout_or_cancel(
            read_timeout,
            cancellation,
            io.read_exact(&mut header),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("read TLS ClientHello header", &error))?;
        let record_len = usize::from(u16::from_be_bytes([header[3], header[4]]));
        if header[0] != 22
            || record_len == 0
            || records
                .len()
                .saturating_add(header.len())
                .saturating_add(record_len)
                > MAX_CLIENT_HELLO_BYTES
        {
            return Err(config_error("TLS ClientHello record is invalid"));
        }
        let mut payload = vec![0; record_len];
        timeout_or_cancel(
            read_timeout,
            cancellation,
            io.read_exact(&mut payload),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("read TLS ClientHello body", &error))?;
        records.extend_from_slice(&header);
        records.extend_from_slice(&payload);
        handshake.extend_from_slice(&payload);

        if handshake.len() >= 4 {
            if handshake[0] != 1 {
                return Err(config_error("first TLS handshake is not ClientHello"));
            }
            let handshake_len = (usize::from(handshake[1]) << 16)
                | (usize::from(handshake[2]) << 8)
                | usize::from(handshake[3]);
            let total = 4usize.saturating_add(handshake_len);
            if total > MAX_CLIENT_HELLO_BYTES {
                return Err(config_error("TLS ClientHello exceeds the configured limit"));
            }
            if handshake.len() >= total {
                return Ok(Bytes::from(records));
            }
        }
    }
}

fn client_hello_requires_tunnel(record: &[u8]) -> bool {
    client_hello_alpn_protocols(record).is_some_and(|protocols| {
        protocols
            .iter()
            .any(|protocol| protocol.as_slice() == b"h2" || protocol.starts_with(b"h3"))
    })
}

fn client_hello_alpn_protocols(record: &[u8]) -> Option<Vec<Vec<u8>>> {
    let payload = collect_client_hello_handshake(record)?;
    if payload.len() < 4 || payload[0] != 1 {
        return None;
    }
    let handshake_len =
        (usize::from(payload[1]) << 16) | (usize::from(payload[2]) << 8) | usize::from(payload[3]);
    let hello = payload.get(4..4usize.checked_add(handshake_len)?)?;
    let mut offset = 2usize.checked_add(32)?;
    let session_len = usize::from(*hello.get(offset)?);
    offset = offset.checked_add(1 + session_len)?;
    let cipher_len = usize::from(u16::from_be_bytes([
        *hello.get(offset)?,
        *hello.get(offset + 1)?,
    ]));
    offset = offset.checked_add(2 + cipher_len)?;
    let compression_len = usize::from(*hello.get(offset)?);
    offset = offset.checked_add(1 + compression_len)?;
    let extensions_len = usize::from(u16::from_be_bytes([
        *hello.get(offset)?,
        *hello.get(offset + 1)?,
    ]));
    offset = offset.checked_add(2)?;
    let extensions = hello.get(offset..offset.checked_add(extensions_len)?)?;
    let mut extension_offset = 0usize;
    while extension_offset + 4 <= extensions.len() {
        let kind = u16::from_be_bytes([
            extensions[extension_offset],
            extensions[extension_offset + 1],
        ]);
        let length = usize::from(u16::from_be_bytes([
            extensions[extension_offset + 2],
            extensions[extension_offset + 3],
        ]));
        extension_offset += 4;
        let data = extensions.get(extension_offset..extension_offset.checked_add(length)?)?;
        extension_offset += length;
        if kind == 16 {
            let list_len = usize::from(u16::from_be_bytes([*data.first()?, *data.get(1)?]));
            let list = data.get(2..2usize.checked_add(list_len)?)?;
            let mut protocols = Vec::new();
            let mut protocol_offset = 0usize;
            while protocol_offset < list.len() {
                let length = usize::from(*list.get(protocol_offset)?);
                protocol_offset += 1;
                let protocol = list.get(protocol_offset..protocol_offset.checked_add(length)?)?;
                protocols.push(protocol.to_vec());
                protocol_offset += length;
            }
            return Some(protocols);
        }
    }
    None
}

/// TLS `ClientHello` 可以跨多个 record 分片。判定是否需要保持 h2/h3 隧道时必须先重组
/// 完整握手，否则只检查首个 record 会把分片的 h2 `ClientHello` 错误送进 HTTP/1 MITM。
fn collect_client_hello_handshake(records: &[u8]) -> Option<Vec<u8>> {
    let mut offset = 0usize;
    let mut handshake = Vec::new();
    while offset < records.len() {
        let header = records.get(offset..offset.checked_add(5)?)?;
        if header[0] != 22 {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([header[3], header[4]]));
        offset = offset.checked_add(5)?;
        let payload = records.get(offset..offset.checked_add(length)?)?;
        handshake.extend_from_slice(payload);
        offset = offset.checked_add(length)?;
        if handshake.len() >= 4 {
            let length = (usize::from(handshake[1]) << 16)
                | (usize::from(handshake[2]) << 8)
                | usize::from(handshake[3]);
            let total = 4usize.checked_add(length)?;
            if handshake.len() >= total {
                handshake.truncate(total);
                return Some(handshake);
            }
        }
    }
    None
}

async fn run_tunnel<A, U>(
    downstream: A,
    upstream: U,
    idle_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (downstream_read, downstream_write) = tokio::io::split(downstream);
    let (upstream_read, upstream_write) = tokio::io::split(upstream);
    let up_cancel = cancellation.clone();
    let down_cancel = cancellation.clone();
    let upstream_copy = copy_direction(downstream_read, upstream_write, idle_timeout, up_cancel);
    let downstream_copy =
        copy_direction(upstream_read, downstream_write, idle_timeout, down_cancel);
    tokio::try_join!(upstream_copy, downstream_copy)?;
    Ok(())
}

async fn copy_direction<R, W>(
    mut reader: R,
    mut writer: W,
    idle_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut copied = 0u64;
    let mut buffer = vec![0; COPY_BUFFER_BYTES];
    loop {
        let read = timeout_or_cancel(
            idle_timeout,
            &cancellation,
            reader.read(&mut buffer),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("read CONNECT tunnel", &error))?;
        if read == 0 {
            timeout_or_cancel(
                idle_timeout,
                &cancellation,
                writer.shutdown(),
                ErrorCode::UpstreamWriteTimeout,
            )
            .await?
            .map_err(|error| ProxyError::io("half-close CONNECT tunnel", &error))?;
            return Ok(copied);
        }
        timeout_or_cancel(
            idle_timeout,
            &cancellation,
            writer.write_all(&buffer[..read]),
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("write CONNECT tunnel", &error))?;
        copied = copied.saturating_add(read as u64);
    }
}

async fn timeout_or_cancel<F, T>(
    duration: Duration,
    cancellation: &CancellationToken,
    future: F,
    timeout_code: ErrorCode,
) -> Result<T>
where
    F: std::future::Future<Output = T>,
{
    tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "forward proxy operation cancelled",
        )),
        result = tokio::time::timeout(duration, future) => result.map_err(|_| ProxyError::new(
            timeout_code,
            format!("forward proxy operation timed out after {} ms", duration.as_millis()),
        )),
    }
}

fn incoming_body(body: Incoming) -> ProxyBody {
    body.map_err(|error| -> BoxError { Box::new(error) })
        .boxed_unsync()
}

fn full_body(value: impl Into<Bytes>) -> ProxyBody {
    Full::new(value.into())
        .map_err(|never| -> BoxError { match never {} })
        .boxed_unsync()
}

fn scheduled_body(
    value: Bytes,
    claimed_length: usize,
    schedule: TrafficSchedule,
    cancellation: &CancellationToken,
) -> ProxyBody {
    if schedule.is_passthrough() {
        return full_body(value);
    }
    PacedBody::new(value, claimed_length, schedule, cancellation.clone())
        .map_err(|error| -> BoxError { Box::new(error) })
        .boxed_unsync()
}

fn empty_response(status: StatusCode) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .body(full_body(Bytes::new()))
        .expect("static response is valid")
}

fn text_response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(full_body(message.to_owned()))
        .expect("static response is valid")
}

fn error_response(error: &ProxyError) -> Response<ProxyBody> {
    let status = match error.code {
        "CONFIG_INVALID" => StatusCode::BAD_REQUEST,
        "UPSTREAM_CONNECT_TIMEOUT" | "UPSTREAM_READ_TIMEOUT" | "UPSTREAM_WRITE_TIMEOUT" => {
            StatusCode::GATEWAY_TIMEOUT
        }
        _ => StatusCode::BAD_GATEWAY,
    };
    text_response(status, &error.message)
}

fn config_error(message: impl Into<String>) -> ProxyError {
    ProxyError::new(ErrorCode::ConfigInvalid, message)
}

fn tls_config_error(error: impl std::fmt::Display) -> ProxyError {
    ProxyError::new(
        ErrorCode::CertificateInvalid,
        format!("MITM TLS configuration failed: {error}"),
    )
}

fn unbracket_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host)
}

fn format_host(host: &str) -> String {
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

fn format_authority(host: &str, port: u16) -> String {
    format!("{}:{port}", format_host(host))
}

#[derive(Debug, Clone, Copy)]
struct Network {
    address: IpAddr,
    prefix: u8,
}

impl Network {
    fn parse(value: &str) -> Option<Self> {
        let (address, prefix) = value.split_once('/')?;
        let address = address.parse::<IpAddr>().ok()?;
        let prefix = prefix.parse::<u8>().ok()?;
        (prefix <= if address.is_ipv4() { 32 } else { 128 }).then_some(Self { address, prefix })
    }

    fn contains(self, candidate: IpAddr) -> bool {
        match (self.address, candidate) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - self.prefix)
                };
                u32::from(network) & mask == u32::from(candidate) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                let mask = if self.prefix == 0 {
                    0
                } else {
                    u128::MAX << (128 - self.prefix)
                };
                u128::from(network) & mask == u128::from(candidate) & mask
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "forward/tests/mod.rs"]
mod tests;
