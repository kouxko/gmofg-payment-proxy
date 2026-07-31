//! Injectable TCP, HTTP/1.1 and pipeline transport.

use std::fmt::{Debug, Formatter};
use std::future::{Future, poll_fn};
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
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf, ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, TryAcquireError, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::fault::{self, FaultAction, ResponseDisposition};
use crate::message::{Message, MessageLimits, RawHeader};
use crate::supervisor::ChannelId;
use crate::tls::ClientTlsAdapter;
use crate::traffic::{
    IntermittentProfile, JitterProfile, PacedBody, PacedBodyError, ThrottleProfile,
    TrafficDirection, TrafficSchedule,
};
use crate::{ErrorCode, ProxyError, Result};

mod raw_http1;

use raw_http1::{
    RawHttp1HeadCapture, ReadRecordingIo, RequestHeadPreservingIo, ResponseHeadPreservingIo,
};

pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T> IoStream for T where T: AsyncRead + AsyncWrite + Unpin + Send {}
pub type BoxIo = Box<dyn IoStream>;
type SharedWriteHalf = Arc<StdMutex<WriteHalf<BoxIo>>>;

struct SplitIo {
    reader: ReadHalf<BoxIo>,
    writer: SharedWriteHalf,
}

impl AsyncRead for SplitIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(context, buffer)
    }
}

impl AsyncWrite for SplitIo {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut writer = self
            .writer
            .lock()
            .expect("downstream HTTP writer mutex poisoned");
        Pin::new(&mut *writer).poll_write(context, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut writer = self
            .writer
            .lock()
            .expect("downstream HTTP writer mutex poisoned");
        Pin::new(&mut *writer).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut writer = self
            .writer
            .lock()
            .expect("downstream HTTP writer mutex poisoned");
        Pin::new(&mut *writer).poll_shutdown(context)
    }
}

#[derive(Clone)]
pub struct InformationalResponseSink {
    writer: SharedWriteHalf,
    write_timeout: Duration,
}

impl Debug for InformationalResponseSink {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InformationalResponseSink")
            .field("write_timeout", &self.write_timeout)
            .finish_non_exhaustive()
    }
}

impl InformationalResponseSink {
    fn new(writer: SharedWriteHalf, write_timeout: Duration) -> Self {
        Self {
            writer,
            write_timeout,
        }
    }

    pub async fn publish(&self, head: Bytes, cancellation: &CancellationToken) -> Result<()> {
        if informational_status(&head).is_none() {
            return Err(ProxyError::new(
                ErrorCode::Internal,
                "only informational HTTP response heads may be published early",
            ));
        }
        let writer = Arc::clone(&self.writer);
        timeout_stage(
            self.write_timeout,
            cancellation,
            async move {
                let mut offset = 0usize;
                while offset < head.len() {
                    let written = poll_fn(|context| {
                        let mut writer = writer
                            .lock()
                            .expect("downstream HTTP writer mutex poisoned");
                        Pin::new(&mut *writer).poll_write(context, &head[offset..])
                    })
                    .await?;
                    if written == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "failed to write informational HTTP response head",
                        ));
                    }
                    offset += written;
                }
                poll_fn(|context| {
                    let mut writer = writer
                        .lock()
                        .expect("downstream HTTP writer mutex poisoned");
                    Pin::new(&mut *writer).poll_flush(context)
                })
                .await
            },
            ErrorCode::Io,
        )
        .await?
        .map_err(|error| ProxyError::io("write informational HTTP response downstream", &error))
    }
}

#[derive(Debug, Default)]
struct RequestWriteTracker {
    body_complete: AtomicBool,
    request_flushed: AtomicBool,
    flushed: Notify,
}

#[derive(Debug, Default)]
struct ResponseWriteTracker {
    response_ready: AtomicBool,
    ready: Notify,
}

impl ResponseWriteTracker {
    fn mark_response_ready(&self) {
        if !self.response_ready.swap(true, Ordering::AcqRel) {
            self.ready.notify_waiters();
        }
    }

    async fn wait_until_ready(&self) {
        loop {
            let notified = self.ready.notified();
            if self.response_ready.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
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
    inner: PacedBody,
    tracker: Arc<RequestWriteTracker>,
}

impl TrackedRequestBody {
    fn new(
        data: Bytes,
        tracker: Arc<RequestWriteTracker>,
        schedule: TrafficSchedule,
        cancellation: CancellationToken,
    ) -> Self {
        let data_len = data.len();
        if data.is_empty() {
            tracker.mark_body_complete();
        }
        Self {
            inner: PacedBody::new(data, data_len, schedule, cancellation),
            tracker,
        }
    }
}

impl Body for TrackedRequestBody {
    type Data = Bytes;
    type Error = PacedBodyError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        let outcome = Pin::new(&mut self.inner).poll_frame(context);
        if matches!(&outcome, Poll::Ready(None))
            || matches!(&outcome, Poll::Ready(Some(Ok(_)))) && self.inner.is_end_stream()
        {
            self.tracker.mark_body_complete();
        }
        outcome
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

struct TrackedIo {
    inner: BoxIo,
    tracker: Arc<RequestWriteTracker>,
    response_head: Arc<StdMutex<RawHttp1HeadCapture>>,
    informational_ready: mpsc::UnboundedSender<()>,
    max_head_bytes: usize,
}

impl Debug for TrackedIo {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrackedIo")
            .field("inner", &"<IoStream>")
            .field("tracker", &self.tracker)
            .field("max_head_bytes", &self.max_head_bytes)
            .finish_non_exhaustive()
    }
}

impl TrackedIo {
    fn new(
        inner: BoxIo,
        tracker: Arc<RequestWriteTracker>,
        response_head: Arc<StdMutex<RawHttp1HeadCapture>>,
        informational_ready: mpsc::UnboundedSender<()>,
        max_head_bytes: usize,
    ) -> Self {
        Self {
            inner,
            tracker,
            response_head,
            informational_ready,
            max_head_bytes,
        }
    }
}

impl AsyncRead for TrackedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(context, buffer);
        if matches!(result, Poll::Ready(Ok(()))) {
            let filled = buffer.filled();
            if filled.len() > before {
                let informational_before;
                let informational_after;
                {
                    let mut response_head = self
                        .response_head
                        .lock()
                        .expect("raw HTTP head capture mutex poisoned");
                    informational_before = response_head.informational.len();
                    response_head.record(&filled[before..], self.max_head_bytes);
                    informational_after = response_head.informational.len();
                }
                if informational_after > informational_before {
                    let _ = self.informational_ready.send(());
                }
            }
        }
        result
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
    inner: PacedBody,
    finish_delay: Option<Pin<Box<tokio::time::Sleep>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentionalWireFault {
    IncorrectContentLength,
    TruncatedResponse,
    StreamAborted,
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
            Self::StreamAborted => ProxyError::new(
                ErrorCode::FaultStreamAborted,
                "response intentionally disconnected during downstream write",
            ),
        }
    }
}

impl WireBody {
    fn new(
        data: Bytes,
        claimed_length: usize,
        schedule: TrafficSchedule,
        cancellation: CancellationToken,
    ) -> Self {
        let finish_delay = (data.len() != claimed_length)
            .then(|| Box::pin(tokio::time::sleep(Duration::from_millis(1))));
        Self {
            inner: PacedBody::new(data, claimed_length, schedule, cancellation),
            finish_delay,
        }
    }
}

impl Body for WireBody {
    type Data = Bytes;
    type Error = PacedBodyError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        match Pin::new(&mut self.inner).poll_frame(context) {
            Poll::Ready(None) => {}
            outcome => return outcome,
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
        self.inner.size_hint()
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
    pub channel: ChannelId,
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
    async fn runtime_fault(&self, _epoch: Uuid, _channel: ChannelId, _error: &ProxyError) {}
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

#[derive(Debug, Clone)]
pub struct UpstreamExchange {
    pub informational_heads: Vec<Bytes>,
    pub final_response: Message,
}

impl From<Message> for UpstreamExchange {
    fn from(final_response: Message) -> Self {
        Self {
            informational_heads: Vec::new(),
            final_response,
        }
    }
}

#[async_trait]
pub trait UpstreamConnector: Debug + Send + Sync {
    async fn send(
        &self,
        request: ForwardRequest,
        actions: &[FaultAction],
        informational: Option<&InformationalResponseSink>,
        cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange>;
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
        informational: Option<&InformationalResponseSink>,
        cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
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
        let schedule = traffic_schedule(actions, TrafficDirection::Upstream)?;
        if schedule.disconnect_after_bytes.is_some() {
            return send_scheduled_upstream_abort(
                &mut io,
                &request.message,
                schedule,
                self.write_timeout,
                cancellation,
            )
            .await
            .map(UpstreamExchange::from);
        }

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
            let wire_request = request.message.reconstruct();
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
            Http1ExchangeConfig {
                schedule,
                write_timeout: self.write_timeout,
                read_timeout: self.read_timeout,
                limits: self.limits,
            },
            informational,
            cancellation,
        )
        .await
    }
}

enum WriteStageOutcome {
    Flushed,
    Response(hyper::Result<Response<Incoming>>),
}

#[derive(Debug, Clone)]
struct Http1ExchangeConfig {
    schedule: TrafficSchedule,
    write_timeout: Duration,
    read_timeout: Duration,
    limits: MessageLimits,
}

async fn publish_new_informational_heads(
    response_head: &StdMutex<RawHttp1HeadCapture>,
    published_count: &mut usize,
    informational: Option<&InformationalResponseSink>,
    cancellation: &CancellationToken,
) -> Result<()> {
    let Some(informational) = informational else {
        return Ok(());
    };
    let heads = response_head
        .lock()
        .expect("raw HTTP response head capture mutex poisoned")
        .informational_heads("upstream response")?;
    for head in heads.iter().skip(*published_count) {
        informational.publish(head.clone(), cancellation).await?;
    }
    *published_count = heads.len();
    Ok(())
}

async fn send_http1_request(
    io: BoxIo,
    request: ForwardRequest,
    config: Http1ExchangeConfig,
    informational: Option<&InformationalResponseSink>,
    cancellation: &CancellationToken,
) -> Result<UpstreamExchange> {
    let Http1ExchangeConfig {
        schedule,
        write_timeout,
        read_timeout,
        limits,
    } = config;
    let effective_write_timeout =
        write_timeout.saturating_add(schedule.estimated_delay(request.message.body.len()));
    let canonical_head = message_wire_head(&request.message)?;
    let io: BoxIo = Box::new(RequestHeadPreservingIo::new(io, canonical_head));
    let tracker = Arc::new(RequestWriteTracker::default());
    let response_head = Arc::new(StdMutex::new(RawHttp1HeadCapture::final_response()));
    let (informational_ready, mut informational_events) = mpsc::unbounded_channel();
    let tracked_io = TrackedIo::new(
        io,
        tracker.clone(),
        Arc::clone(&response_head),
        informational_ready,
        raw_head_capture_limit(limits),
    );
    let mut http1 = client_http1::Builder::new();
    http1.title_case_headers(true);
    let (mut sender, connection) = http1
        .handshake(TokioIo::new(tracked_io))
        .await
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
    let connection_task = ConnectionTask::spawn(connection);

    let result = async {
        timeout_stage(
            effective_write_timeout,
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
                schedule,
                cancellation.clone(),
            ))
            .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
        *outgoing.headers_mut() = headers;

        let mut response_future = Box::pin(sender.send_request(outgoing));
        let mut published_informational = 0usize;
        let write_outcome = timeout_stage(
            effective_write_timeout,
            cancellation,
            async {
                loop {
                    publish_new_informational_heads(
                        &response_head,
                        &mut published_informational,
                        informational,
                        cancellation,
                    )
                    .await?;
                    tokio::select! {
                        response = &mut response_future => {
                            break Ok(WriteStageOutcome::Response(response));
                        }
                        () = tracker.wait_until_flushed() => {
                            break Ok(WriteStageOutcome::Flushed);
                        }
                        Some(()) = informational_events.recv() => {}
                    }
                }
            },
            ErrorCode::UpstreamWriteTimeout,
        )
        .await??;
        let response = match write_outcome {
            WriteStageOutcome::Response(response) => response,
            WriteStageOutcome::Flushed => {
                timeout_stage(
                    read_timeout,
                    cancellation,
                    async {
                        loop {
                            publish_new_informational_heads(
                                &response_head,
                                &mut published_informational,
                                informational,
                                cancellation,
                            )
                            .await?;
                            tokio::select! {
                                response = &mut response_future => break Ok(response),
                                Some(()) = informational_events.recv() => {}
                            }
                        }
                    },
                    ErrorCode::UpstreamReadTimeout,
                )
                .await??
            }
        }
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        publish_new_informational_heads(
            &response_head,
            &mut published_informational,
            informational,
            cancellation,
        )
        .await?;

        let (parts, body) = response.into_parts();
        let body = timeout_stage(
            read_timeout,
            cancellation,
            collect_limited(body, limits.max_body_bytes),
            ErrorCode::UpstreamReadTimeout,
        )
        .await??;
        let raw_head = response_head
            .lock()
            .expect("raw HTTP response head capture mutex poisoned")
            .required_head("upstream response")?;
        let message = Message::from_raw_http1_head(&raw_head, body)?;
        if message.http_status() != Some(parts.status.as_u16()) {
            return Err(ProxyError::new(
                ErrorCode::Io,
                "captured upstream HTTP status does not match Hyper's final response",
            ));
        }
        message.validate(limits)?;
        let informational_heads = response_head
            .lock()
            .expect("raw HTTP response head capture mutex poisoned")
            .informational_heads("upstream response")?
            .into_iter()
            .skip(published_informational)
            .collect();
        Ok(UpstreamExchange {
            informational_heads,
            final_response: message,
        })
    }
    .await;

    connection_task.shutdown().await;
    result
}

async fn send_scheduled_upstream_abort(
    io: &mut BoxIo,
    message: &Message,
    schedule: TrafficSchedule,
    write_timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Message> {
    let after_bytes = schedule
        .disconnect_after_bytes
        .expect("disconnect schedule was checked");
    if after_bytes >= message.body.len() {
        return Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "upstream disconnect offset must be smaller than request body",
        ));
    }
    let wire = message.reconstruct();
    let header_end = wire
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .ok_or_else(|| ProxyError::new(ErrorCode::Internal, "request headers are incomplete"))?;
    timeout_stage(
        write_timeout,
        cancellation,
        io.write_all(&wire[..header_end]),
        ErrorCode::UpstreamWriteTimeout,
    )
    .await?
    .map_err(|error| ProxyError::io("write upstream request headers", &error))?;

    let mut body = PacedBody::new(
        message.body.clone(),
        message.body.len(),
        schedule,
        cancellation.clone(),
    );
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Ok(data) = frame.into_data() {
                    timeout_stage(
                        write_timeout,
                        cancellation,
                        io.write_all(&data),
                        ErrorCode::UpstreamWriteTimeout,
                    )
                    .await?
                    .map_err(|error| ProxyError::io("write paced upstream body", &error))?;
                }
            }
            Err(PacedBodyError::Disconnected) => {
                let _ = io.shutdown().await;
                return Err(ProxyError::new(
                    ErrorCode::FaultStreamAborted,
                    "request intentionally disconnected during upstream write",
                ));
            }
            Err(PacedBodyError::Cancelled) => {
                return Err(ProxyError::new(
                    ErrorCode::FaultExecutionCancelled,
                    "weak-network request cancelled",
                ));
            }
        }
    }
    Err(ProxyError::new(
        ErrorCode::Internal,
        "upstream disconnect schedule completed without disconnecting",
    ))
}

fn traffic_schedule(
    actions: &[FaultAction],
    direction: TrafficDirection,
) -> Result<TrafficSchedule> {
    let mut schedule = TrafficSchedule::default();
    for action in actions {
        match action {
            FaultAction::Jitter {
                minimum,
                maximum,
                scope,
                seed,
            } => {
                schedule.jitter = Some(JitterProfile {
                    minimum: *minimum,
                    maximum: *maximum,
                    scope: *scope,
                });
                schedule.seed = *seed;
            }
            FaultAction::Throttle {
                bytes_per_second,
                chunk_bytes,
                direction: action_direction,
            } if *action_direction == direction => {
                if *bytes_per_second == 0 || *chunk_bytes == 0 {
                    return Err(ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        "throttle rate and chunk size must be greater than zero",
                    ));
                }
                schedule.throttle = Some(ThrottleProfile {
                    bytes_per_second: *bytes_per_second,
                    chunk_bytes: *chunk_bytes,
                });
            }
            FaultAction::Intermittent {
                available,
                blocked,
                direction: action_direction,
            } if *action_direction == direction => {
                if available.is_zero() || blocked.is_zero() {
                    return Err(ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        "intermittent windows must be greater than zero",
                    ));
                }
                schedule.intermittent = Some(IntermittentProfile {
                    available: *available,
                    blocked: *blocked,
                });
            }
            FaultAction::DisconnectDuringWrite {
                after_bytes,
                direction: action_direction,
            } if *action_direction == direction => {
                schedule.disconnect_after_bytes = Some(*after_bytes);
            }
            _ => {}
        }
    }
    Ok(schedule)
}

#[derive(Debug, Clone)]
pub struct ConnectionService {
    pub acceptor: Arc<dyn ConnectionAcceptor>,
    pub upstream: Arc<dyn UpstreamConnector>,
    pub ports: Arc<dyn PipelinePorts>,
    pub clock: Arc<dyn Clock>,
    pub admission: ConnectionAdmission,
    pub limits: MessageLimits,
    /// Covers the inbound TLS handshake and the downstream request body.
    pub read_timeout: Duration,
    /// Covers each downstream response write stage.
    pub write_timeout: Duration,
}

struct RequestWireState<'a> {
    raw_request_head: &'a StdMutex<RawHttp1HeadCapture>,
    canonical_response_head: &'a StdMutex<Option<Bytes>>,
    informational_response_sink: &'a InformationalResponseSink,
    raw_tail: &'a StdMutex<Option<Bytes>>,
    intentional_wire_fault: &'a StdMutex<Option<IntentionalWireFault>>,
}

/// Shared per-epoch admission control. All channel listeners clone the same
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
        channel: ChannelId,
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
                        channel: channel.clone(),
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
        let raw_request_head = Arc::new(StdMutex::new(RawHttp1HeadCapture::default()));
        let handler_request_head = Arc::clone(&raw_request_head);
        let canonical_response_head = Arc::new(StdMutex::new(None::<Bytes>));
        let handler_response_head = Arc::clone(&canonical_response_head);
        let (reader, writer) = tokio::io::split(io);
        let shared_writer = Arc::new(StdMutex::new(writer));
        let split_io: BoxIo = Box::new(SplitIo {
            reader,
            writer: Arc::clone(&shared_writer),
        });
        let informational_response_sink =
            InformationalResponseSink::new(shared_writer, self.write_timeout);
        let handler_informational_sink = informational_response_sink.clone();
        let intentional_wire_fault = Arc::new(StdMutex::new(None::<IntentionalWireFault>));
        let handler_wire_fault = Arc::clone(&intentional_wire_fault);
        let handler_error = Arc::new(StdMutex::new(None::<ProxyError>));
        let service_error = Arc::clone(&handler_error);
        let response_write = Arc::new(ResponseWriteTracker::default());
        let handler_response_write = Arc::clone(&response_write);
        let handler = service_fn(move |request| {
            let service = service.clone();
            let context = context.clone();
            let cancellation = request_cancel.clone();
            let raw_tail = Arc::clone(&handler_tail);
            let raw_request_head = Arc::clone(&handler_request_head);
            let canonical_response_head = Arc::clone(&handler_response_head);
            let informational_response_sink = handler_informational_sink.clone();
            let intentional_wire_fault = Arc::clone(&handler_wire_fault);
            let service_error = Arc::clone(&service_error);
            let response_write = Arc::clone(&handler_response_write);
            async move {
                let wire = RequestWireState {
                    raw_request_head: &raw_request_head,
                    canonical_response_head: &canonical_response_head,
                    informational_response_sink: &informational_response_sink,
                    raw_tail: &raw_tail,
                    intentional_wire_fault: &intentional_wire_fault,
                };
                let result = service
                    .handle_request(request, &context, &cancellation, &wire)
                    .await;
                if result.is_ok() {
                    response_write.mark_response_ready();
                }
                result.map_err(|error| {
                    let wire_error = io::Error::other(error.to_string());
                    *service_error.lock().expect("handler error mutex poisoned") = Some(error);
                    wire_error
                })
            }
        });
        let response_preserving_io: BoxIo = Box::new(ResponseHeadPreservingIo::new(
            split_io,
            canonical_response_head,
        ));
        let recording_io = ReadRecordingIo::new(
            response_preserving_io,
            raw_request_head,
            raw_head_capture_limit(self.limits),
        );
        let mut connection = Box::pin(
            server_http1::Builder::new()
                .keep_alive(false)
                .max_headers(self.limits.max_headers)
                .serve_connection(TokioIo::new(recording_io), handler)
                .without_shutdown(),
        );
        let initial = tokio::select! {
            () = cancellation.cancelled() => {
                return Err(ProxyError::new(
                    ErrorCode::ProxyStopped,
                    "proxy stopped while connection was active",
                ));
            }
            result = &mut connection => Some(result),
            () = response_write.wait_until_ready() => None,
        };
        let result = match initial {
            Some(result) => result,
            None => {
                timeout_stage(
                    self.write_timeout,
                    &cancellation,
                    &mut connection,
                    ErrorCode::Io,
                )
                .await?
            }
        };
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
        let mut io = parts.io.into_inner().into_inner();
        let tail = raw_tail.lock().expect("raw tail mutex poisoned").take();
        finish_downstream_write(
            &mut io,
            tail,
            self.write_timeout,
            &cancellation,
            &intentional_wire_fault,
        )
        .await
    }

    async fn handle_request(
        &self,
        request: Request<Incoming>,
        context: &ConnectionContext,
        cancellation: &CancellationToken,
        wire: &RequestWireState<'_>,
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
        let raw_head = wire
            .raw_request_head
            .lock()
            .expect("raw HTTP request head capture mutex poisoned")
            .required_head("downstream request")?;
        let mut message = Message::from_raw_http1_head(&raw_head, body)?;
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
                    body,
                } => {
                    let message = fault::mock_response(*status, headers, body.clone());
                    return response_from_disposition(
                        ResponseDisposition::Send {
                            message,
                            schedule: TrafficSchedule::default(),
                        },
                        wire.canonical_response_head,
                        wire.raw_tail,
                        wire.intentional_wire_fault,
                        cancellation,
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
        let upstream_exchange = self
            .upstream
            .send(
                forward,
                &request_actions,
                Some(wire.informational_response_sink),
                cancellation,
            )
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
        for head in upstream_exchange.informational_heads {
            wire.informational_response_sink
                .publish(head, cancellation)
                .await?;
        }
        let mut upstream_response = upstream_exchange.final_response;
        let response_actions = self.ports.response(context, &mut upstream_response).await?;
        let disposition =
            fault::apply_response_actions(upstream_response, &response_actions, cancellation)
                .await?;
        response_from_disposition(
            disposition,
            wire.canonical_response_head,
            wire.raw_tail,
            wire.intentional_wire_fault,
            cancellation,
        )
    }
}

async fn finish_downstream_write(
    io: &mut BoxIo,
    tail: Option<Bytes>,
    write_timeout: Duration,
    cancellation: &CancellationToken,
    intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
) -> Result<()> {
    if let Some(tail) = tail
        && let Err(error) = timeout_stage(
            write_timeout,
            cancellation,
            io.write_all(&tail),
            ErrorCode::Io,
        )
        .await
        .and_then(|result| {
            result.map_err(|error| ProxyError::io("write short content-length tail", &error))
        })
    {
        return Err(intentional_fault_or(error, intentional_wire_fault));
    }
    if let Err(error) = timeout_stage(write_timeout, cancellation, io.flush(), ErrorCode::Io)
        .await
        .and_then(|result| {
            result.map_err(|error| ProxyError::io("flush client connection", &error))
        })
    {
        return Err(intentional_fault_or(error, intentional_wire_fault));
    }
    if let Err(error) = timeout_stage(write_timeout, cancellation, io.shutdown(), ErrorCode::Io)
        .await
        .and_then(|result| {
            result.map_err(|error| ProxyError::io("shutdown client connection", &error))
        })
    {
        return Err(intentional_fault_or(error, intentional_wire_fault));
    }
    if let Some(fault) = *intentional_wire_fault
        .lock()
        .expect("intentional wire fault mutex poisoned")
    {
        return Err(fault.error());
    }
    Ok(())
}

fn intentional_fault_or(
    error: ProxyError,
    intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
) -> ProxyError {
    if error.code == ErrorCode::ProxyStopped.as_str() {
        return error;
    }
    intentional_wire_fault
        .lock()
        .expect("intentional wire fault mutex poisoned")
        .map_or(error, IntentionalWireFault::error)
}

fn response_from_disposition(
    disposition: ResponseDisposition,
    canonical_response_head: &StdMutex<Option<Bytes>>,
    raw_tail: &StdMutex<Option<Bytes>>,
    intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
    cancellation: &CancellationToken,
) -> Result<Response<WireBody>> {
    let (mut message, mut body, mut schedule, disposition_fault) = match disposition {
        ResponseDisposition::Send { message, schedule } => {
            let body = message.body.clone();
            (message, body, schedule, None)
        }
        ResponseDisposition::Drop => {
            return Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "response intentionally dropped",
            ));
        }
        ResponseDisposition::Truncate {
            message,
            bytes,
            schedule,
        } => {
            let body = message.body.slice(..bytes);
            (
                message,
                body,
                schedule,
                Some(IntentionalWireFault::TruncatedResponse),
            )
        }
    };
    let status = parse_response_status(&message.start_line)?;
    let claimed_length = message
        .declared_content_length()
        .unwrap_or(message.body.len());
    let scheduled_abort = if let Some(after_bytes) = schedule.disconnect_after_bytes.take() {
        if after_bytes >= body.len() {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "downstream disconnect offset must be smaller than response body",
            ));
        }
        body = body.slice(..after_bytes);
        Some(IntentionalWireFault::StreamAborted)
    } else {
        None
    };
    let disposition_fault = disposition_fault.or(scheduled_abort).or_else(|| {
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
    message.remove_header("connection");
    message.headers.push(RawHeader::new(
        Bytes::from_static(b"Connection"),
        Bytes::from_static(b"close"),
    ));
    if message.declared_content_length().is_none() {
        message.set_content_length(message.body.len());
    }
    *canonical_response_head
        .lock()
        .expect("canonical HTTP response head mutex poisoned") = Some(message_wire_head(&message)?);
    let mut response = Response::builder()
        .status(status)
        .version(http::Version::HTTP_11)
        .body(WireBody::new(
            body,
            claimed_length,
            schedule,
            cancellation.clone(),
        ))
        .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
    *response.headers_mut() = message.header_map()?;
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

fn informational_status(head: &[u8]) -> Option<u16> {
    let line_end = head.windows(2).position(|window| window == b"\r\n")?;
    let line = std::str::from_utf8(&head[..line_end]).ok()?;
    let status = line.split_ascii_whitespace().nth(1)?.parse::<u16>().ok()?;
    (100..200).contains(&status).then_some(status)
}

fn raw_head_capture_limit(limits: MessageLimits) -> usize {
    // `max_total_header_bytes` counts only names and values. Reserve the
    // delimiters plus a bounded start-line so the recorder can retain the
    // complete head that Hyper has already accepted.
    limits
        .max_total_header_bytes
        .saturating_add(limits.max_headers.saturating_mul(4))
        .saturating_add(8 * 1024)
        .saturating_add(4)
}

fn message_wire_head(message: &Message) -> Result<Bytes> {
    let wire = message.reconstruct();
    let end = wire
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .ok_or_else(|| ProxyError::new(ErrorCode::Internal, "HTTP request head is incomplete"))?;
    Ok(wire.slice(..end))
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
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn response_head_capture_skips_informational_heads_and_keeps_the_final_head() {
        let mut capture = RawHttp1HeadCapture::final_response();
        capture.record(
            b"HTTP/1.1 100 Continue\r\nX-Info:\t first \r\n\r\n\
HTTP/1.1 103 Early Hints\r\nLink: </style.css>\r\n\r\n\
HTTP/1.1 299 Vendor Final\r\nX-Final: yes\r\n\r\nbody",
            1024,
        );

        assert_eq!(
            capture.required_head("response").expect("final head"),
            Bytes::from_static(b"HTTP/1.1 299 Vendor Final\r\nX-Final: yes\r\n\r\n")
        );
    }

    #[test]
    fn raw_head_capture_reports_limit_exhaustion_instead_of_falling_back() {
        let mut capture = RawHttp1HeadCapture::default();
        capture.record(b"GET / HTTP/1.1\r\nX-Test: value\r\n\r\n", 12);

        let error = capture
            .required_head("request")
            .expect_err("truncated capture must fail closed");
        assert_eq!(error.code, ErrorCode::HeaderLimitExceeded.as_str());
    }

    #[derive(Debug)]
    struct FixedResponseConnector {
        body: Bytes,
        declared_content_length: Option<usize>,
    }

    #[derive(Debug)]
    struct UnusedAcceptor;

    #[derive(Debug, Clone, Copy)]
    enum PendingWriteStage {
        Tail,
        Flush,
        Shutdown,
    }

    #[derive(Debug)]
    struct PendingWriteIo(PendingWriteStage);

    impl AsyncRead for PendingWriteIo {
        fn poll_read(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    impl AsyncWrite for PendingWriteIo {
        fn poll_write(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            match self.0 {
                PendingWriteStage::Tail => Poll::Pending,
                PendingWriteStage::Flush | PendingWriteStage::Shutdown => {
                    Poll::Ready(Ok(buffer.len()))
                }
            }
        }

        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            match self.0 {
                PendingWriteStage::Flush => Poll::Pending,
                PendingWriteStage::Tail | PendingWriteStage::Shutdown => Poll::Ready(Ok(())),
            }
        }

        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            match self.0 {
                PendingWriteStage::Tail | PendingWriteStage::Flush => Poll::Ready(Ok(())),
                PendingWriteStage::Shutdown => Poll::Pending,
            }
        }
    }

    #[async_trait]
    impl ConnectionAcceptor for UnusedAcceptor {
        async fn accept(
            &self,
            _io: BoxIo,
            _context: &ConnectionContext,
        ) -> Result<AcceptedConnection> {
            unreachable!("run_connection_inner does not invoke the acceptor")
        }
    }

    #[async_trait]
    impl UpstreamConnector for FixedResponseConnector {
        async fn send(
            &self,
            _request: ForwardRequest,
            _actions: &[FaultAction],
            _informational: Option<&InformationalResponseSink>,
            _cancellation: &CancellationToken,
        ) -> Result<UpstreamExchange> {
            let mut message =
                Message::response(StatusCode::OK, &HeaderMap::new(), self.body.clone());
            if let Some(length) = self.declared_content_length {
                message.set_content_length(length);
            }
            Ok(message.into())
        }
    }

    fn downstream_test_service(
        body: Bytes,
        declared_content_length: Option<usize>,
        write_timeout: Duration,
    ) -> ConnectionService {
        ConnectionService {
            acceptor: Arc::new(UnusedAcceptor),
            upstream: Arc::new(FixedResponseConnector {
                body,
                declared_content_length,
            }),
            ports: Arc::new(NoopPipelinePorts),
            clock: Arc::new(SystemClock),
            admission: ConnectionAdmission::new(1).expect("valid test capacity"),
            limits: MessageLimits::default(),
            read_timeout: Duration::from_secs(1),
            write_timeout,
        }
    }

    fn downstream_test_context() -> ConnectionContext {
        ConnectionContext {
            runtime_epoch: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
            channel: ChannelId::new("alpha").expect("valid test channel ID"),
            peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12_345),
            accepted_at: SystemTime::now(),
            tls_peer: None,
        }
    }

    async fn write_test_request(client: &mut tokio::io::DuplexStream) {
        client
            .write_all(b"POST / HTTP/1.1\r\nHost: proxy.test\r\nContent-Length: 0\r\n\r\n")
            .await
            .expect("write test request");
    }

    #[tokio::test]
    async fn intentional_content_length_and_truncation_faults_have_stable_classifications() {
        let cases = [
            (
                ResponseDisposition::Send {
                    message: {
                        let mut message = Message::response(
                            StatusCode::OK,
                            &HeaderMap::new(),
                            Bytes::from_static(b"body"),
                        );
                        message.set_content_length(24);
                        message
                    },
                    schedule: TrafficSchedule::default(),
                },
                IntentionalWireFault::IncorrectContentLength,
                ErrorCode::IncorrectContentLength,
            ),
            (
                ResponseDisposition::Send {
                    message: {
                        let mut message = Message::response(
                            StatusCode::OK,
                            &HeaderMap::new(),
                            Bytes::from_static(b"body"),
                        );
                        message.set_content_length(2);
                        message
                    },
                    schedule: TrafficSchedule::default(),
                },
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
                    schedule: TrafficSchedule::default(),
                },
                IntentionalWireFault::TruncatedResponse,
                ErrorCode::TruncatedResponse,
            ),
        ];

        for (disposition, expected_fault, expected_code) in cases {
            let canonical_head = StdMutex::new(None);
            let raw_tail = StdMutex::new(None);
            let fault = StdMutex::new(None);
            response_from_disposition(
                disposition,
                &canonical_head,
                &raw_tail,
                &fault,
                &CancellationToken::new(),
            )
            .expect("intentional wire response should be constructed");
            let actual = fault
                .lock()
                .expect("intentional wire fault mutex poisoned")
                .expect("intentional wire fault marker");
            assert_eq!(actual, expected_fault);
            assert_eq!(actual.error().code, expected_code.as_str());
        }
    }

    #[tokio::test]
    async fn downstream_mid_body_disconnect_sends_exact_prefix_and_keeps_declared_length() {
        let canonical_head = StdMutex::new(None);
        let raw_tail = StdMutex::new(None);
        let fault = StdMutex::new(None);
        let response = response_from_disposition(
            ResponseDisposition::Send {
                message: Message::response(
                    StatusCode::OK,
                    &HeaderMap::new(),
                    Bytes::from_static(b"abcdefgh"),
                ),
                schedule: TrafficSchedule {
                    disconnect_after_bytes: Some(3),
                    ..TrafficSchedule::default()
                },
            },
            &canonical_head,
            &raw_tail,
            &fault,
            &CancellationToken::new(),
        )
        .expect("downstream abort response");
        assert_eq!(
            response.headers().get("content-length").unwrap(),
            http::HeaderValue::from_static("8")
        );
        let mut body = response.into_body();
        let prefix = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(prefix, Bytes::from_static(b"abc"));
        assert!(body.frame().await.is_none());
        assert_eq!(
            *fault.lock().expect("intentional fault mutex poisoned"),
            Some(IntentionalWireFault::StreamAborted)
        );
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

    #[tokio::test]
    async fn downstream_response_write_respects_write_timeout() {
        let (mut client, server) = tokio::io::duplex(128);
        write_test_request(&mut client).await;
        let service = downstream_test_service(
            Bytes::from(vec![b'x'; 4 * 1024]),
            None,
            Duration::from_millis(10),
        );

        let error = tokio::time::timeout(
            Duration::from_secs(1),
            service.run_connection_inner(
                Box::new(server),
                &downstream_test_context(),
                CancellationToken::new(),
            ),
        )
        .await
        .expect("downstream write must terminate within the configured timeout")
        .expect_err("a downstream client that does not read must time out");

        assert_eq!(error.code, ErrorCode::Io.as_str());
        assert!(error.message.contains("timed out after 10 ms"));
    }

    #[tokio::test]
    async fn downstream_response_write_stops_when_supervisor_cancels() {
        let (mut client, server) = tokio::io::duplex(128);
        write_test_request(&mut client).await;
        let service = downstream_test_service(
            Bytes::from(vec![b'x'; 4 * 1024]),
            None,
            Duration::from_secs(30),
        );
        let cancellation = CancellationToken::new();
        let stop = cancellation.clone();
        let context = downstream_test_context();

        let ((), result) = tokio::join!(
            async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                stop.cancel();
            },
            service.run_connection_inner(Box::new(server), &context, cancellation,)
        );
        let error = result.expect_err("supervisor cancellation must stop the downstream write");

        assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
    }

    #[tokio::test]
    async fn incorrect_content_length_tail_write_is_bounded() {
        let (mut client, server) = tokio::io::duplex(256);
        write_test_request(&mut client).await;
        let service = downstream_test_service(
            Bytes::from(vec![b'x'; 4 * 1024]),
            Some(1),
            Duration::from_millis(10),
        );

        let error = tokio::time::timeout(
            Duration::from_secs(1),
            service.run_connection_inner(
                Box::new(server),
                &downstream_test_context(),
                CancellationToken::new(),
            ),
        )
        .await
        .expect("incorrect content-length tail write must be bounded")
        .expect_err("intentional incorrect content-length remains a terminal fault");

        assert_eq!(error.code, ErrorCode::IncorrectContentLength.as_str());
    }

    #[tokio::test]
    async fn incorrect_content_length_tail_write_stops_when_supervisor_cancels() {
        let mut io: BoxIo = Box::new(PendingWriteIo(PendingWriteStage::Tail));
        let cancellation = CancellationToken::new();
        let stop = cancellation.clone();
        let intentional_fault = StdMutex::new(Some(IntentionalWireFault::IncorrectContentLength));

        let ((), result) = tokio::join!(
            async move {
                tokio::time::sleep(Duration::from_millis(5)).await;
                stop.cancel();
            },
            finish_downstream_write(
                &mut io,
                Some(Bytes::from_static(b"tail")),
                Duration::from_secs(30),
                &cancellation,
                &intentional_fault,
            )
        );
        let error = result.expect_err("supervisor cancellation must stop the raw tail write");

        assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
    }

    #[tokio::test]
    async fn downstream_flush_and_shutdown_each_respect_write_timeout() {
        for stage in [PendingWriteStage::Flush, PendingWriteStage::Shutdown] {
            let mut io: BoxIo = Box::new(PendingWriteIo(stage));
            let error = finish_downstream_write(
                &mut io,
                None,
                Duration::from_millis(5),
                &CancellationToken::new(),
                &StdMutex::new(None),
            )
            .await
            .expect_err("a stalled downstream write stage must time out");

            assert_eq!(error.code, ErrorCode::Io.as_str());
            assert!(error.message.contains("timed out after 5 ms"));
        }
    }
}
