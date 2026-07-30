//! Injectable TCP, HTTP/1.1 and pipeline transport.

use std::convert::Infallible;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, Method, Request, Response, StatusCode, Uri};
use http_body_util::BodyExt;
use hyper::body::{Body, Frame, Incoming, SizeHint};
use hyper::client::conn::http1 as client_http1;
use hyper::server::conn::http1 as server_http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tokio::task::{JoinHandle, JoinSet};
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

#[derive(Debug, Default)]
struct RequestWriteTracker {
    body_complete: AtomicBool,
    request_flushed: AtomicBool,
    flushed: Notify,
}

impl RequestWriteTracker {
    fn mark_body_complete(&self) {
        self.body_complete.store(true, Ordering::Release);
    }

    fn mark_request_flushed(&self) {
        if self.body_complete.load(Ordering::Acquire)
            && !self.request_flushed.swap(true, Ordering::AcqRel)
        {
            self.flushed.notify_waiters();
        }
    }

    async fn wait_until_flushed(&self) {
        loop {
            let notified = self.flushed.notified();
            if self.request_flushed.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

#[derive(Debug)]
struct TrackedRequestBody {
    data: Option<Bytes>,
    tracker: Arc<RequestWriteTracker>,
}

impl TrackedRequestBody {
    fn new(data: Bytes, tracker: Arc<RequestWriteTracker>) -> Self {
        if data.is_empty() {
            tracker.mark_body_complete();
        }
        Self {
            data: (!data.is_empty()).then_some(data),
            tracker,
        }
    }
}

impl Body for TrackedRequestBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        if let Some(data) = self.data.take() {
            self.tracker.mark_body_complete();
            return Poll::Ready(Some(Ok(Frame::data(data))));
        }
        self.tracker.mark_body_complete();
        Poll::Ready(None)
    }

    fn is_end_stream(&self) -> bool {
        self.data.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        let remaining = self.data.as_ref().map_or(0, Bytes::len);
        SizeHint::with_exact(u64::try_from(remaining).unwrap_or(u64::MAX))
    }
}

struct TrackedIo {
    inner: BoxIo,
    tracker: Arc<RequestWriteTracker>,
}

impl Debug for TrackedIo {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrackedIo")
            .field("inner", &"<IoStream>")
            .field("tracker", &self.tracker)
            .finish()
    }
}

impl TrackedIo {
    fn new(inner: BoxIo, tracker: Arc<RequestWriteTracker>) -> Self {
        Self { inner, tracker }
    }
}

impl AsyncRead for TrackedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for TrackedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.inner).poll_flush(context) {
            Poll::Ready(Ok(())) => {
                self.tracker.mark_request_flushed();
                Poll::Ready(Ok(()))
            }
            outcome => outcome,
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

#[derive(Debug)]
struct ConnectionTask {
    handle: Option<JoinHandle<()>>,
}

impl ConnectionTask {
    fn spawn(connection: impl Future<Output = hyper::Result<()>> + Send + 'static) -> Self {
        let handle = tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(?error, "upstream HTTP/1 connection ended");
            }
        });
        Self {
            handle: Some(handle),
        }
    }

    async fn shutdown(mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        handle.abort();
        if let Err(error) = handle.await
            && !error.is_cancelled()
        {
            tracing::error!(?error, "upstream HTTP/1 connection task failed");
        }
    }
}

impl Drop for ConnectionTask {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

#[derive(Debug)]
struct WireBody {
    data: Option<Bytes>,
    claimed_length: u64,
    finish_delay: Option<Pin<Box<tokio::time::Sleep>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentionalWireFault {
    IncorrectContentLength,
    TruncatedResponse,
}

impl IntentionalWireFault {
    fn error(self) -> ProxyError {
        match self {
            Self::IncorrectContentLength => ProxyError::new(
                ErrorCode::IncorrectContentLength,
                "response sent with intentionally incorrect content-length",
            ),
            Self::TruncatedResponse => ProxyError::new(
                ErrorCode::TruncatedResponse,
                "response intentionally truncated before completion",
            ),
        }
    }
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
        wait_for_injected_timeout(actions, InjectedTimeoutStage::Connect, cancellation).await?;

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

        wait_for_injected_timeout(actions, InjectedTimeoutStage::Write, cancellation).await?;

        let close_after_request_write = actions.iter().any(|action| {
            matches!(
                action,
                FaultAction::DropResponse {
                    read_upstream: false
                }
            )
        });
        let inject_read_timeout = injected_timeout(actions, InjectedTimeoutStage::Read).is_some();
        if close_after_request_write || inject_read_timeout {
            let wire_request = request.message.reconstruct_title_case_headers();
            timeout_stage(
                self.write_timeout,
                cancellation,
                io.write_all(&wire_request),
                ErrorCode::UpstreamWriteTimeout,
            )
            .await?
            .map_err(|error| ProxyError::io("write injected upstream request", &error))?;
            timeout_stage(
                self.write_timeout,
                cancellation,
                io.flush(),
                ErrorCode::UpstreamWriteTimeout,
            )
            .await?
            .map_err(|error| ProxyError::io("flush injected upstream request", &error))?;

            if close_after_request_write {
                timeout_stage(
                    self.write_timeout,
                    cancellation,
                    io.shutdown(),
                    ErrorCode::UpstreamWriteTimeout,
                )
                .await?
                .map_err(|error| ProxyError::io("close injected upstream request", &error))?;
                return Err(ProxyError::new(
                    ErrorCode::ClientDisconnected,
                    "upstream request intentionally closed after complete write",
                ));
            }

            wait_for_injected_timeout(actions, InjectedTimeoutStage::Read, cancellation).await?;
            return Err(ProxyError::new(
                ErrorCode::Internal,
                "injected read timeout unexpectedly completed",
            ));
        }

        send_http1_request(
            io,
            request,
            self.write_timeout,
            self.read_timeout,
            self.limits,
            cancellation,
        )
        .await
    }
}

enum WriteStageOutcome {
    Flushed,
    Response(hyper::Result<Response<Incoming>>),
}

async fn send_http1_request(
    io: BoxIo,
    request: ForwardRequest,
    write_timeout: Duration,
    read_timeout: Duration,
    limits: MessageLimits,
    cancellation: &CancellationToken,
) -> Result<Message> {
    let tracker = Arc::new(RequestWriteTracker::default());
    let tracked_io = TrackedIo::new(io, tracker.clone());
    let mut http1 = client_http1::Builder::new();
    http1.title_case_headers(true);
    let (mut sender, connection) = http1
        .handshake(TokioIo::new(tracked_io))
        .await
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
    let connection_task = ConnectionTask::spawn(connection);

    let result = async {
        timeout_stage(
            write_timeout,
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
            .body(TrackedRequestBody::new(
                request.message.body.clone(),
                tracker.clone(),
            ))
            .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
        *outgoing.headers_mut() = headers;

        let mut response_future = Box::pin(sender.send_request(outgoing));
        let write_outcome = timeout_stage(
            write_timeout,
            cancellation,
            async {
                tokio::select! {
                    response = &mut response_future => WriteStageOutcome::Response(response),
                    () = tracker.wait_until_flushed() => WriteStageOutcome::Flushed,
                }
            },
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?;
        let response = match write_outcome {
            WriteStageOutcome::Response(response) => response,
            WriteStageOutcome::Flushed => {
                timeout_stage(
                    read_timeout,
                    cancellation,
                    &mut response_future,
                    ErrorCode::UpstreamReadTimeout,
                )
                .await?
            }
        }
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;

        let (parts, body) = response.into_parts();
        let body = timeout_stage(
            read_timeout,
            cancellation,
            collect_limited(body, limits.max_body_bytes),
            ErrorCode::UpstreamReadTimeout,
        )
        .await??;
        let message = Message::response(parts.status, &parts.headers, body);
        message.validate(limits)?;
        Ok(message)
    }
    .await;

    connection_task.shutdown().await;
    result
}

#[derive(Debug, Clone)]
pub struct ConnectionService {
    pub acceptor: Arc<dyn ConnectionAcceptor>,
    pub upstream: Arc<dyn UpstreamConnector>,
    pub ports: Arc<dyn PipelinePorts>,
    pub clock: Arc<dyn Clock>,
    pub admission: ConnectionAdmission,
    pub limits: MessageLimits,
    /// Covers the inbound TLS handshake and the Payment App request body.
    pub read_timeout: Duration,
}

/// Shared per-epoch admission control. Both channel listeners clone the same
/// instance so their combined pre-handshake and active connection count stays
/// within one configured capacity.
#[derive(Debug, Clone)]
pub struct ConnectionAdmission {
    permits: Arc<Semaphore>,
}

impl ConnectionAdmission {
    pub fn new(capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "connection capacity must be greater than zero",
            ));
        }
        Ok(Self {
            permits: Arc::new(Semaphore::new(capacity)),
        })
    }

    fn try_acquire(&self) -> std::result::Result<OwnedSemaphorePermit, TryAcquireError> {
        Arc::clone(&self.permits).try_acquire_owned()
    }
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
                    let permit = match self.admission.try_acquire() {
                        Ok(permit) => permit,
                        Err(error) => {
                            tracing::warn!(
                                ?channel,
                                %peer_addr,
                                ?error,
                                "proxy connection rejected because runtime capacity is exhausted"
                            );
                            drop(io);
                            continue;
                        }
                    };
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
                        let _permit = permit;
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
        let accepted = timeout_stage(
            self.read_timeout,
            &cancellation,
            self.acceptor.accept(io, &context),
            ErrorCode::TlsHandshakeFailed,
        )
        .await
        .and_then(std::convert::identity);
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
        let intentional_wire_fault = Arc::new(StdMutex::new(None::<IntentionalWireFault>));
        let handler_wire_fault = Arc::clone(&intentional_wire_fault);
        let handler_error = Arc::new(StdMutex::new(None::<ProxyError>));
        let service_error = Arc::clone(&handler_error);
        let handler = service_fn(move |request| {
            let service = service.clone();
            let context = context.clone();
            let cancellation = request_cancel.clone();
            let raw_tail = Arc::clone(&handler_tail);
            let intentional_wire_fault = Arc::clone(&handler_wire_fault);
            let service_error = Arc::clone(&service_error);
            async move {
                service
                    .handle_request(
                        request,
                        &context,
                        &cancellation,
                        &raw_tail,
                        &intentional_wire_fault,
                    )
                    .await
                    .map_err(|error| {
                        let wire_error = io::Error::other(error.to_string());
                        *service_error.lock().expect("handler error mutex poisoned") = Some(error);
                        wire_error
                    })
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
                let parts = match result {
                    Ok(parts) => parts,
                    Err(error) => {
                        let original = handler_error
                            .lock()
                            .expect("handler error mutex poisoned")
                            .take();
                        if let Some(original) = original {
                            return Err(original);
                        }
                        if let Some(fault) = *intentional_wire_fault
                            .lock()
                            .expect("intentional wire fault mutex poisoned")
                        {
                            return Err(fault.error());
                        }
                        return Err(ProxyError::new(
                            ErrorCode::Io,
                            format!("HTTP/1.1 connection failed: {error}"),
                        ));
                    }
                };
                let mut io = parts.io.into_inner();
                let tail = raw_tail.lock().expect("raw tail mutex poisoned").take();
                if let Some(tail) = tail
                    && let Err(error) = io.write_all(&tail).await
                {
                    if let Some(fault) = *intentional_wire_fault
                        .lock()
                        .expect("intentional wire fault mutex poisoned")
                    {
                        return Err(fault.error());
                    }
                    return Err(ProxyError::io("write short content-length tail", &error));
                }
                if let Err(error) = io.shutdown().await {
                    if let Some(fault) = *intentional_wire_fault
                        .lock()
                        .expect("intentional wire fault mutex poisoned")
                    {
                        return Err(fault.error());
                    }
                    return Err(ProxyError::io("shutdown client connection", &error));
                }
                if let Some(fault) = *intentional_wire_fault
                    .lock()
                    .expect("intentional wire fault mutex poisoned")
                {
                    return Err(fault.error());
                }
                Ok(())
            },
        }
    }

    async fn handle_request(
        &self,
        request: Request<Incoming>,
        context: &ConnectionContext,
        cancellation: &CancellationToken,
        raw_tail: &StdMutex<Option<Bytes>>,
        intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
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
                    return response_from_disposition(
                        ResponseDisposition::Send(message),
                        raw_tail,
                        intentional_wire_fault,
                    );
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
        response_from_disposition(disposition, raw_tail, intentional_wire_fault)
    }
}

fn response_from_disposition(
    disposition: ResponseDisposition,
    raw_tail: &StdMutex<Option<Bytes>>,
    intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
) -> Result<Response<WireBody>> {
    let (message, body, disposition_fault) = match disposition {
        ResponseDisposition::Send(message) => {
            let body = message.body.clone();
            (message, body, None)
        }
        ResponseDisposition::Drop => {
            return Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "response intentionally dropped",
            ));
        }
        ResponseDisposition::Truncate { message, bytes } => {
            let body = message.body.slice(..bytes);
            (message, body, Some(IntentionalWireFault::TruncatedResponse))
        }
    };
    let status = parse_response_status(&message.start_line)?;
    let claimed_length = message
        .declared_content_length()
        .unwrap_or(message.body.len());
    let disposition_fault = disposition_fault.or_else(|| {
        (claimed_length != body.len()).then_some(IntentionalWireFault::IncorrectContentLength)
    });
    if let Some(fault) = disposition_fault {
        *intentional_wire_fault
            .lock()
            .expect("intentional wire fault mutex poisoned") = Some(fault);
    }
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

#[derive(Clone, Copy)]
enum InjectedTimeoutStage {
    Connect,
    Write,
    Read,
}

impl InjectedTimeoutStage {
    const fn error_code(self) -> ErrorCode {
        match self {
            Self::Connect => ErrorCode::UpstreamConnectTimeout,
            Self::Write => ErrorCode::UpstreamWriteTimeout,
            Self::Read => ErrorCode::UpstreamReadTimeout,
        }
    }
}

fn injected_timeout(actions: &[FaultAction], stage: InjectedTimeoutStage) -> Option<Duration> {
    actions.iter().find_map(|action| match (stage, action) {
        (InjectedTimeoutStage::Connect, FaultAction::UpstreamConnectTimeout(timeout))
        | (InjectedTimeoutStage::Write, FaultAction::UpstreamWriteTimeout(timeout))
        | (InjectedTimeoutStage::Read, FaultAction::UpstreamReadTimeout(timeout)) => Some(*timeout),
        _ => None,
    })
}

async fn wait_for_injected_timeout(
    actions: &[FaultAction],
    stage: InjectedTimeoutStage,
    cancellation: &CancellationToken,
) -> Result<()> {
    let Some(timeout) = injected_timeout(actions, stage) else {
        return Ok(());
    };
    wait_for_timeout(timeout, cancellation, stage.error_code()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn intentional_content_length_and_truncation_faults_have_stable_classifications() {
        let cases = [
            (
                ResponseDisposition::Send({
                    let mut message = Message::response(
                        StatusCode::OK,
                        &HeaderMap::new(),
                        Bytes::from_static(b"body"),
                    );
                    message.set_content_length(24);
                    message
                }),
                IntentionalWireFault::IncorrectContentLength,
                ErrorCode::IncorrectContentLength,
            ),
            (
                ResponseDisposition::Send({
                    let mut message = Message::response(
                        StatusCode::OK,
                        &HeaderMap::new(),
                        Bytes::from_static(b"body"),
                    );
                    message.set_content_length(2);
                    message
                }),
                IntentionalWireFault::IncorrectContentLength,
                ErrorCode::IncorrectContentLength,
            ),
            (
                ResponseDisposition::Truncate {
                    message: Message::response(
                        StatusCode::OK,
                        &HeaderMap::new(),
                        Bytes::from_static(b"body"),
                    ),
                    bytes: 2,
                },
                IntentionalWireFault::TruncatedResponse,
                ErrorCode::TruncatedResponse,
            ),
        ];

        for (disposition, expected_fault, expected_code) in cases {
            let raw_tail = StdMutex::new(None);
            let fault = StdMutex::new(None);
            response_from_disposition(disposition, &raw_tail, &fault)
                .expect("intentional wire response should be constructed");
            let actual = fault
                .lock()
                .expect("intentional wire fault mutex poisoned")
                .expect("intentional wire fault marker");
            assert_eq!(actual, expected_fault);
            assert_eq!(actual.error().code, expected_code.as_str());
        }
    }

    // ACTION-003~005, TEST-FAULT:
    // the runtime must wait for each rule's exact duration, not the global connector timeout.
    #[tokio::test]
    async fn injected_timeouts_use_the_duration_carried_by_each_rule_action() {
        let cases = [
            (
                InjectedTimeoutStage::Connect,
                FaultAction::UpstreamConnectTimeout(Duration::from_millis(2)),
                ErrorCode::UpstreamConnectTimeout,
                2,
            ),
            (
                InjectedTimeoutStage::Write,
                FaultAction::UpstreamWriteTimeout(Duration::from_millis(3)),
                ErrorCode::UpstreamWriteTimeout,
                3,
            ),
            (
                InjectedTimeoutStage::Read,
                FaultAction::UpstreamReadTimeout(Duration::from_millis(4)),
                ErrorCode::UpstreamReadTimeout,
                4,
            ),
        ];

        for (stage, action, code, milliseconds) in cases {
            let error = wait_for_injected_timeout(&[action], stage, &CancellationToken::new())
                .await
                .expect_err("configured timeout must terminate with its stage error");
            assert_eq!(error.code, code.as_str());
            assert_eq!(
                error.message,
                format!("injected timeout after {milliseconds} ms")
            );
        }
    }

    #[tokio::test]
    async fn injected_timeout_only_applies_to_its_matching_stage() {
        let action = FaultAction::UpstreamReadTimeout(Duration::from_mins(1));
        wait_for_injected_timeout(
            &[action],
            InjectedTimeoutStage::Connect,
            &CancellationToken::new(),
        )
        .await
        .expect("read timeout must not affect connect stage");
    }

    #[tokio::test]
    async fn injected_timeouts_stop_immediately_when_proxy_is_cancelled() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        for (stage, action) in [
            (
                InjectedTimeoutStage::Connect,
                FaultAction::UpstreamConnectTimeout(Duration::from_mins(1)),
            ),
            (
                InjectedTimeoutStage::Write,
                FaultAction::UpstreamWriteTimeout(Duration::from_mins(1)),
            ),
            (
                InjectedTimeoutStage::Read,
                FaultAction::UpstreamReadTimeout(Duration::from_mins(1)),
            ),
        ] {
            let error = wait_for_injected_timeout(&[action], stage, &cancellation)
                .await
                .expect_err("proxy stop must cancel every injected timeout");
            assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
        }
    }
}
