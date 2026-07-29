//! Injectable TCP, HTTP/1.1 and pipeline transport.

use std::convert::Infallible;
use std::fmt::Debug;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, Method, Request, Response, StatusCode, Uri};
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Frame, Incoming, SizeHint};
use hyper::client::conn::http1 as client_http1;
use hyper::server::conn::http1 as server_http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::fault::{self, FaultAction, ResponseDisposition};
use crate::message::{self, Message, MessageLimits};
use crate::supervisor::Channel;
use crate::tls::ClientTlsAdapter;
use crate::{ErrorCode, ProxyError, Result};

pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> IoStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}
pub type BoxIo = Box<dyn IoStream>;

#[derive(Debug)]
struct WireBody {
    data: Option<Bytes>,
    claimed_length: u64,
    finish_delay: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl WireBody {
    fn new(data: Bytes, claimed_length: usize) -> Self {
        let finish_delay = (data.len() != claimed_length)
            .then(|| Box::pin(tokio::time::sleep(Duration::from_millis(1))));
        Self {
            data: Some(data),
            claimed_length: u64::try_from(claimed_length).unwrap_or(u64::MAX),
            finish_delay,
        }
    }
}

impl Body for WireBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        if let Some(data) = self.data.take() {
            return Poll::Ready(Some(Ok(Frame::data(data))));
        }
        if let Some(delay) = self.finish_delay.as_mut() {
            if delay.as_mut().poll(context).is_pending() {
                return Poll::Pending;
            }
            self.finish_delay = None;
        }
        Poll::Ready(None)
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.claimed_length)
    }
}

#[async_trait]
pub trait ConnectionAcceptor: Debug + Send + Sync {
    async fn accept(&self, io: BoxIo, context: &ConnectionContext) -> Result<AcceptedConnection>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsPeerIdentity {
    pub sha256_fingerprint: String,
    pub subject_summary: String,
}

pub struct AcceptedConnection {
    pub io: BoxIo,
    pub tls_peer: Option<TlsPeerIdentity>,
}

impl Debug for AcceptedConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcceptedConnection")
            .field("tls_peer", &self.tls_peer)
            .finish_non_exhaustive()
    }
}

#[async_trait]
pub trait BoundListener: Debug + Send + Sync {
    fn local_addr(&self) -> io::Result<SocketAddr>;
    async fn accept(&self) -> io::Result<(BoxIo, SocketAddr)>;
}

#[async_trait]
pub trait ListenerBinder: Debug + Send + Sync {
    async fn bind(&self, address: SocketAddr) -> io::Result<Arc<dyn BoundListener>>;
}

#[derive(Debug, Default)]
pub struct TokioListenerBinder;

#[async_trait]
impl ListenerBinder for TokioListenerBinder {
    async fn bind(&self, address: SocketAddr) -> io::Result<Arc<dyn BoundListener>> {
        Ok(Arc::new(TokioBoundListener(
            TcpListener::bind(address).await?,
        )))
    }
}

#[derive(Debug)]
struct TokioBoundListener(TcpListener);

#[async_trait]
impl BoundListener for TokioBoundListener {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }

    async fn accept(&self) -> io::Result<(BoxIo, SocketAddr)> {
        let (stream, address) = self.0.accept().await?;
        stream.set_nodelay(true)?;
        Ok((Box::new(stream), address))
    }
}

#[async_trait]
pub trait Clock: Debug + Send + Sync {
    fn now(&self) -> SystemTime;
    async fn sleep(&self, duration: Duration);
}

#[derive(Debug, Default)]
pub struct SystemClock;

#[async_trait]
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionContext {
    pub runtime_epoch: Uuid,
    pub connection_id: Uuid,
    pub channel: Channel,
    pub peer_addr: SocketAddr,
    pub accepted_at: SystemTime,
    pub tls_peer: Option<TlsPeerIdentity>,
}

/// Synchronous, handshake-safe policy surface used from rustls certificate
/// verification. Implementations must not await or block on UI subscribers.
pub trait HandshakePolicy: Debug + Send + Sync {
    fn reject_tls_handshake(
        &self,
        _context: &ConnectionContext,
        _peer: &TlsPeerIdentity,
    ) -> Result<bool> {
        Ok(false)
    }
}

/// Application-facing hooks. Implementations must not block on UI subscribers.
#[async_trait]
pub trait PipelinePorts: HandshakePolicy {
    async fn runtime_stopping(&self, _epoch: Uuid) {}
    async fn connection_opened(&self, _context: &ConnectionContext) {}
    async fn request(
        &self,
        _context: &ConnectionContext,
        _message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        Ok(Vec::new())
    }
    async fn response(
        &self,
        _context: &ConnectionContext,
        _message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        Ok(Vec::new())
    }
    async fn connection_closed(&self, _context: &ConnectionContext, _result: &Result<()>) {}
    async fn runtime_fault(&self, _epoch: Uuid, _channel: Channel, _error: &ProxyError) {}
}

#[derive(Debug, Default)]
pub struct NoopPipelinePorts;
impl HandshakePolicy for NoopPipelinePorts {}
impl PipelinePorts for NoopPipelinePorts {}

#[derive(Debug, Clone)]
pub struct ForwardRequest {
    pub method: Method,
    pub uri: Uri,
    pub message: Message,
}

#[async_trait]
pub trait UpstreamConnector: Debug + Send + Sync {
    async fn send(
        &self,
        request: ForwardRequest,
        actions: &[FaultAction],
        cancellation: &CancellationToken,
    ) -> Result<Message>;
}

#[derive(Debug, Clone)]
pub struct HyperUpstreamConnector {
    pub address: SocketAddr,
    /// TLS SNI and certificate hostname.
    pub host: String,
    /// HTTP Host header, including a non-default port when configured.
    pub host_header: String,
    pub rewrite_host: bool,
    pub tls: Option<ClientTlsAdapter>,
    pub connect_timeout: Duration,
    pub write_timeout: Duration,
    pub read_timeout: Duration,
    pub limits: MessageLimits,
}

#[async_trait]
impl UpstreamConnector for HyperUpstreamConnector {
    async fn send(
        &self,
        mut request: ForwardRequest,
        actions: &[FaultAction],
        cancellation: &CancellationToken,
    ) -> Result<Message> {
        if actions
            .iter()
            .any(|action| matches!(action, FaultAction::UpstreamConnectTimeout))
        {
            wait_for_timeout(
                self.connect_timeout,
                cancellation,
                ErrorCode::UpstreamConnectTimeout,
            )
            .await?;
        }

        request
            .message
            .normalize_for_forward(&self.host_header, self.rewrite_host);
        let tcp = timeout_stage(
            self.connect_timeout,
            cancellation,
            TcpStream::connect(self.address),
            ErrorCode::UpstreamConnectTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("connect upstream", &error))?;
        tcp.set_nodelay(true)
            .map_err(|error| ProxyError::io("configure upstream", &error))?;
        let mut io: BoxIo = Box::new(tcp);
        if let Some(tls) = &self.tls {
            io = timeout_stage(
                self.connect_timeout,
                cancellation,
                tls.connect(&self.host, io),
                ErrorCode::UpstreamConnectTimeout,
            )
            .await??;
        }

        let (mut sender, connection) = client_http1::handshake(TokioIo::new(io))
            .await
            .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        let connection_task = tokio::spawn(connection);

        if actions
            .iter()
            .any(|action| matches!(action, FaultAction::UpstreamWriteTimeout))
        {
            connection_task.abort();
            wait_for_timeout(
                self.write_timeout,
                cancellation,
                ErrorCode::UpstreamWriteTimeout,
            )
            .await?;
        }
        timeout_stage(
            self.write_timeout,
            cancellation,
            sender.ready(),
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;

        let headers = request.message.header_map()?;
        let mut outgoing = Request::builder()
            .method(request.method)
            .uri(request.uri)
            .version(http::Version::HTTP_11)
            .body(Full::new(request.message.body.clone()))
            .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
        *outgoing.headers_mut() = headers;
        let response = timeout_stage(
            self.read_timeout,
            cancellation,
            sender.send_request(outgoing),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;

        if actions
            .iter()
            .any(|action| matches!(action, FaultAction::UpstreamReadTimeout))
        {
            connection_task.abort();
            wait_for_timeout(
                self.read_timeout,
                cancellation,
                ErrorCode::UpstreamReadTimeout,
            )
            .await?;
        }
        if actions.iter().any(|action| {
            matches!(
                action,
                FaultAction::DropResponse {
                    read_upstream: false
                }
            )
        }) {
            connection_task.abort();
            return Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "upstream response intentionally abandoned",
            ));
        }

        let (parts, body) = response.into_parts();
        let body = timeout_stage(
            self.read_timeout,
            cancellation,
            collect_limited(body, self.limits.max_body_bytes),
            ErrorCode::UpstreamReadTimeout,
        )
        .await??;
        connection_task.abort();
        let message = Message::response(parts.status, &parts.headers, body);
        message.validate(self.limits)?;
        Ok(message)
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionService {
    pub acceptor: Arc<dyn ConnectionAcceptor>,
    pub upstream: Arc<dyn UpstreamConnector>,
    pub ports: Arc<dyn PipelinePorts>,
    pub clock: Arc<dyn Clock>,
    pub limits: MessageLimits,
    pub read_timeout: Duration,
}

impl ConnectionService {
    pub async fn run_listener(
        &self,
        listener: Arc<dyn BoundListener>,
        channel: Channel,
        epoch: Uuid,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                accepted = listener.accept() => {
                    let (io, peer_addr) = accepted
                        .map_err(|error| ProxyError::io("accept connection", &error))?;
                    let context = ConnectionContext {
                        runtime_epoch: epoch,
                        connection_id: Uuid::new_v4(),
                        channel,
                        peer_addr,
                        accepted_at: self.clock.now(),
                        tls_peer: None,
                    };
                    let service = self.clone();
                    let child_cancel = cancellation.child_token();
                    connections.spawn(async move {
                        service.run_connection(io, context, child_cancel).await;
                    });
                }
                Some(joined) = connections.join_next(), if !connections.is_empty() => {
                    if let Err(error) = joined {
                        tracing::warn!(?error, "proxy connection task failed");
                    }
                }
            }
        }
        cancellation.cancel();
        while connections.join_next().await.is_some() {}
        Ok(())
    }

    async fn run_connection(
        &self,
        io: BoxIo,
        mut context: ConnectionContext,
        cancellation: CancellationToken,
    ) {
        let accepted = self.acceptor.accept(io, &context).await;
        let result = match accepted {
            Ok(accepted) => {
                context.tls_peer = accepted.tls_peer;
                self.ports.connection_opened(&context).await;
                self.run_connection_inner(accepted.io, &context, cancellation)
                    .await
            }
            Err(error) => Err(error),
        };
        self.ports.connection_closed(&context, &result).await;
    }

    async fn run_connection_inner(
        &self,
        io: BoxIo,
        context: &ConnectionContext,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let service = self.clone();
        let context = context.clone();
        let request_cancel = cancellation.clone();
        let raw_tail = Arc::new(StdMutex::new(None::<Bytes>));
        let handler_tail = Arc::clone(&raw_tail);
        let handler = service_fn(move |request| {
            let service = service.clone();
            let context = context.clone();
            let cancellation = request_cancel.clone();
            let raw_tail = Arc::clone(&handler_tail);
            async move {
                service
                    .handle_request(request, &context, &cancellation, &raw_tail)
                    .await
                    .map_err(|error| io::Error::other(error.to_string()))
            }
        });
        let connection = server_http1::Builder::new()
            .keep_alive(false)
            .max_headers(self.limits.max_headers)
            .serve_connection(TokioIo::new(io), handler)
            .without_shutdown();
        tokio::select! {
            () = cancellation.cancelled() => Err(ProxyError::new(
                ErrorCode::ProxyStopped,
                "proxy stopped while connection was active",
            )),
            result = connection => {
                let parts = result.map_err(|error| {
                    ProxyError::new(ErrorCode::Io, format!("HTTP/1.1 connection failed: {error}"))
                })?;
                let mut io = parts.io.into_inner();
                let tail = raw_tail.lock().expect("raw tail mutex poisoned").take();
                if let Some(tail) = tail {
                    io.write_all(&tail).await.map_err(|error| {
                        ProxyError::io("write short content-length tail", &error)
                    })?;
                }
                io.shutdown().await.map_err(|error| {
                    ProxyError::io("shutdown client connection", &error)
                })
            },
        }
    }

    async fn handle_request(
        &self,
        request: Request<Incoming>,
        context: &ConnectionContext,
        cancellation: &CancellationToken,
        raw_tail: &StdMutex<Option<Bytes>>,
    ) -> Result<Response<WireBody>> {
        let (parts, body) = request.into_parts();
        validate_headers(&parts.headers, self.limits)?;
        let body = timeout_stage(
            self.read_timeout,
            cancellation,
            collect_limited(body, self.limits.max_body_bytes),
            ErrorCode::Io,
        )
        .await??;
        let mut message = Message::request(&parts.method, &parts.uri, &parts.headers, body);
        message.validate(self.limits)?;
        let request_actions = self.ports.request(context, &mut message).await?;

        for action in &request_actions {
            match action {
                FaultAction::Delay(duration) => {
                    tokio::select! {
                        () = cancellation.cancelled() => return Err(ProxyError::new(
                            ErrorCode::ProxyStopped,
                            "proxy stopped during request delay",
                        )),
                        () = self.clock.sleep(*duration) => {}
                    }
                }
                FaultAction::DisconnectBeforeUpstream => {
                    return Err(ProxyError::new(
                        ErrorCode::ClientDisconnected,
                        "request intentionally disconnected before upstream",
                    ));
                }
                FaultAction::MockResponse {
                    status,
                    headers,
                    shift_jis_body,
                } => {
                    let message = fault::mock_response(*status, headers, shift_jis_body)?;
                    return response_from_disposition(ResponseDisposition::Send(message), raw_tail);
                }
                FaultAction::RejectTls => {
                    return Err(ProxyError::new(
                        ErrorCode::TlsHandshakeFailed,
                        "TLS intentionally rejected",
                    ));
                }
                _ => {}
            }
        }

        let forward = ForwardRequest {
            method: parts.method,
            uri: parts.uri,
            message,
        };
        let mut upstream_response = self
            .upstream
            .send(forward, &request_actions, cancellation)
            .await?;
        if request_actions.iter().any(|action| {
            matches!(
                action,
                FaultAction::DropResponse {
                    read_upstream: true
                }
            )
        }) {
            return Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "upstream response intentionally dropped after complete read",
            ));
        }
        let response_actions = self.ports.response(context, &mut upstream_response).await?;
        let disposition =
            fault::apply_response_actions(upstream_response, &response_actions, cancellation)
                .await?;
        response_from_disposition(disposition, raw_tail)
    }
}

fn response_from_disposition(
    disposition: ResponseDisposition,
    raw_tail: &StdMutex<Option<Bytes>>,
) -> Result<Response<WireBody>> {
    let (message, body) = match disposition {
        ResponseDisposition::Send(message) => {
            let body = message.body.clone();
            (message, body)
        }
        ResponseDisposition::Drop => {
            return Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "response intentionally dropped",
            ));
        }
        ResponseDisposition::Truncate { message, bytes } => {
            let body = message.body.slice(..bytes);
            (message, body)
        }
    };
    let status = parse_response_status(&message.start_line)?;
    let claimed_length = message
        .declared_content_length()
        .unwrap_or(message.body.len());
    let body = if claimed_length < body.len() {
        let tail = body.slice(claimed_length..);
        *raw_tail.lock().expect("raw tail mutex poisoned") = Some(tail);
        body.slice(..claimed_length)
    } else {
        body
    };
    let mut response = Response::builder()
        .status(status)
        .version(http::Version::HTTP_11)
        .body(WireBody::new(body, claimed_length))
        .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
    *response.headers_mut() = message.header_map()?;
    message::force_connection_close(response.headers_mut());
    if message.declared_content_length().is_none() {
        message::content_length(response.headers_mut(), message.body.len())?;
    }
    Ok(response)
}

fn parse_response_status(start_line: &str) -> Result<StatusCode> {
    let value = start_line
        .split_ascii_whitespace()
        .nth(1)
        .ok_or_else(|| ProxyError::new(ErrorCode::Internal, "response status is missing"))?;
    StatusCode::from_bytes(value.as_bytes())
        .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))
}

async fn collect_limited(mut body: Incoming, limit: usize) -> Result<Bytes> {
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        if let Ok(data) = frame.into_data() {
            if bytes.len().saturating_add(data.len()) > limit {
                return Err(ProxyError::new(
                    ErrorCode::BodyTooLarge,
                    format!("body exceeds {limit} bytes"),
                ));
            }
            bytes.extend_from_slice(&data);
        }
    }
    Ok(Bytes::from(bytes))
}

fn validate_headers(headers: &HeaderMap, limits: MessageLimits) -> Result<()> {
    if headers.len() > limits.max_headers {
        return Err(ProxyError::new(
            ErrorCode::HeaderLimitExceeded,
            "too many headers",
        ));
    }
    let mut total = 0usize;
    for (name, value) in headers {
        total = total.saturating_add(name.as_str().len() + value.as_bytes().len());
        if name.as_str().len() > limits.max_header_name_bytes
            || value.as_bytes().len() > limits.max_header_value_bytes
            || total > limits.max_total_header_bytes
        {
            return Err(ProxyError::new(
                ErrorCode::HeaderLimitExceeded,
                "header size limit exceeded",
            ));
        }
    }
    Ok(())
}

async fn timeout_stage<F, T>(
    timeout: Duration,
    cancellation: &CancellationToken,
    future: F,
    code: ErrorCode,
) -> Result<T>
where
    F: Future<Output = T>,
{
    tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "proxy operation cancelled",
        )),
        result = tokio::time::timeout(timeout, future) => result.map_err(|_| {
            ProxyError::new(code, format!("operation timed out after {} ms", timeout.as_millis()))
        }),
    }
}

async fn wait_for_timeout(
    timeout: Duration,
    cancellation: &CancellationToken,
    code: ErrorCode,
) -> Result<()> {
    tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "proxy operation cancelled",
        )),
        () = tokio::time::sleep(timeout) => Err(ProxyError::new(
            code,
            format!("injected timeout after {} ms", timeout.as_millis()),
        )),
    }
}
