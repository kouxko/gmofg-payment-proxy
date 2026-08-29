//! All-interface, stateless Streamable HTTP transport backed by official `rmcp`.

mod capabilities;
mod error;

use std::{convert::Infallible, fmt, io, net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::{BodyExt as _, Full, LengthLimitError, Limited};
use hyper::{Method, Request, Response, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpSocket, TcpStream},
    sync::Semaphore,
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tower_service::Service as _;

use super::{
    backend::McpBackend,
    protocol::{self, McpHandler},
};
pub(crate) use capabilities::{
    Ipv6BindOutcome, McpIpCapability, McpTransportCapabilities, McpTransportWarningCode,
};
use error::{TransportError, response as transport_error_response};

pub const MCP_ADDRESS: &str = "0.0.0.0:17653";
pub const MCP_IPV6_ADDRESS: &str = "[::]:17653";
const MCP_PORT: u16 = 17653;
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
// rmcp emits successful tool data in both structured content and its compatibility envelope.
// Cover two exact maximum logical read outputs plus bounded JSON-RPC/envelope serialization slack;
// the protocol layer remains the authority that rejects logical output above 8 MiB.
const RESPONSE_ENVELOPE_SLACK_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize =
    protocol::MAX_LOGICAL_OUTPUT_BYTES * 2 + RESPONSE_ENVELOPE_SLACK_BYTES;
const MAX_CONNECTIONS: usize = 16;
const MAX_CONCURRENT_REQUESTS: usize = 32;
// The protocol layer owns exact per-tool deadlines and permits create to run for 30 seconds.
// Keep the transport bounded without racing that inner deadline or its response serialization.
const TRANSPORT_REQUEST_DEADLINE: Duration = Duration::from_secs(35);
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(5);
const DUAL_STACK_PROBE_DEADLINE: Duration = Duration::from_secs(1);
const DUAL_STACK_PROBE: &[u8] = b"mcp-dual-stack-probe";

#[derive(Clone)]
pub struct McpServer {
    inner: Arc<ServerInner>,
}

struct ServerInner {
    local_addr: SocketAddr,
    transport_capabilities: Arc<McpTransportCapabilities>,
    cancellation: CancellationToken,
    task: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for McpServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("McpServer")
            .field("local_addr", &self.inner.local_addr)
            .field("transport_capabilities", &self.inner.transport_capabilities)
            .field("cancelled", &self.inner.cancellation.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl McpServer {
    pub async fn start(backend: Arc<dyn McpBackend>) -> io::Result<Self> {
        let (listeners, local_addr, ipv6) = bind_production_listeners().await?;
        Ok(Self::start_with_listeners(
            listeners,
            local_addr,
            McpTransportCapabilities::production(ipv6),
            backend,
        ))
    }

    #[cfg(test)]
    async fn start_on(address: &str, backend: Arc<dyn McpBackend>) -> io::Result<Self> {
        let listener = TcpListener::bind(address).await?;
        let local_addr = listener.local_addr()?;
        Ok(Self::start_with_listeners(
            vec![listener],
            local_addr,
            McpTransportCapabilities::test(local_addr),
            backend,
        ))
    }

    fn start_with_listeners(
        listeners: Vec<TcpListener>,
        local_addr: SocketAddr,
        transport_capabilities: McpTransportCapabilities,
        backend: Arc<dyn McpBackend>,
    ) -> Self {
        let cancellation = CancellationToken::new();
        let transport_capabilities = Arc::new(transport_capabilities);
        let task = tokio::spawn(run(
            listeners,
            backend,
            Arc::clone(&transport_capabilities),
            cancellation.clone(),
        ));
        Self {
            inner: Arc::new(ServerInner {
                local_addr,
                transport_capabilities,
                cancellation,
                task: std::sync::Mutex::new(Some(task)),
            }),
        }
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.inner.local_addr
    }

    pub(crate) fn transport_capabilities(&self) -> Arc<McpTransportCapabilities> {
        Arc::clone(&self.inner.transport_capabilities)
    }

    pub fn cancel(&self) {
        self.inner.cancellation.cancel();
    }

    pub async fn shutdown(&self) {
        self.cancel();
        let task = self
            .inner
            .task
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some(mut task) = task else {
            return;
        };
        if timeout(SHUTDOWN_DEADLINE, &mut task).await.is_err() {
            task.abort();
            let _ = task.await;
            tracing::warn!("MCP shutdown exceeded bounded deadline; task aborted");
        }
    }
}

impl Drop for ServerInner {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self
            .task
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}

async fn bind_production_listeners() -> io::Result<(Vec<TcpListener>, SocketAddr, Ipv6BindOutcome)>
{
    let binding = bind_production_listeners_with(&SystemMcpListenerBinder).await?;
    Ok((binding.listeners, binding.local_addr, binding.ipv6))
}

pub(super) struct ProductionBinding<L> {
    pub(super) listeners: Vec<L>,
    pub(super) local_addr: SocketAddr,
    pub(super) ipv6: Ipv6BindOutcome,
}

#[async_trait]
pub(super) trait McpListenerBinder: Sync {
    type Listener: Send + Sync;

    fn bind_ipv4(&self) -> io::Result<Self::Listener>;
    fn local_addr(&self, listener: &Self::Listener) -> io::Result<SocketAddr>;
    async fn probe_ipv4_listener_for_ipv6(&self, listener: &Self::Listener, port: u16) -> bool;
    fn bind_ipv6(&self) -> io::Result<Self::Listener>;
}

pub(super) async fn bind_production_listeners_with<B: McpListenerBinder>(
    binder: &B,
) -> io::Result<ProductionBinding<B::Listener>> {
    let ipv4 = binder.bind_ipv4().map_err(|error| {
        io::Error::new(
            error.kind(),
            "IPV4_BIND_FAILED: MCP IPv4 listener bind failed",
        )
    })?;
    let local_addr = binder.local_addr(&ipv4)?;

    if binder
        .probe_ipv4_listener_for_ipv6(&ipv4, local_addr.port())
        .await
    {
        return Ok(ProductionBinding {
            listeners: vec![ipv4],
            local_addr,
            ipv6: Ipv6BindOutcome::DualStackCovered,
        });
    }

    match binder.bind_ipv6() {
        Ok(ipv6) => Ok(ProductionBinding {
            listeners: vec![ipv4, ipv6],
            local_addr,
            ipv6: Ipv6BindOutcome::Independent,
        }),
        Err(error) if ipv6_is_unsupported(&error) => {
            tracing::warn!(error_kind = ?error.kind(), "MCP IPv6 is unsupported; IPv4 service remains available");
            Ok(ProductionBinding {
                listeners: vec![ipv4],
                local_addr,
                ipv6: Ipv6BindOutcome::Unsupported,
            })
        }
        Err(error) => {
            tracing::warn!(error_kind = ?error.kind(), "MCP IPv6 bind degraded; IPv4 service remains available");
            Ok(ProductionBinding {
                listeners: vec![ipv4],
                local_addr,
                ipv6: Ipv6BindOutcome::Degraded,
            })
        }
    }
}

struct SystemMcpListenerBinder;

#[async_trait]
impl McpListenerBinder for SystemMcpListenerBinder {
    type Listener = TcpListener;

    fn bind_ipv4(&self) -> io::Result<Self::Listener> {
        bind_listener(MCP_ADDRESS, false)
    }

    fn local_addr(&self, listener: &Self::Listener) -> io::Result<SocketAddr> {
        listener.local_addr()
    }

    async fn probe_ipv4_listener_for_ipv6(&self, listener: &Self::Listener, port: u16) -> bool {
        probe_ipv4_listener_for_ipv6(listener, port).await
    }

    fn bind_ipv6(&self) -> io::Result<Self::Listener> {
        bind_listener(MCP_IPV6_ADDRESS, true)
    }
}

fn bind_listener(address: &str, ipv6: bool) -> io::Result<TcpListener> {
    let address = address
        .parse::<SocketAddr>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let socket = if ipv6 {
        TcpSocket::new_v6()?
    } else {
        TcpSocket::new_v4()?
    };
    socket.bind(address)?;
    socket.listen(1024)
}

async fn probe_ipv4_listener_for_ipv6(listener: &TcpListener, port: u16) -> bool {
    let probe = async {
        let connect = TcpStream::connect(("::1", port));
        let accept = listener.accept();
        let (mut client, (mut accepted, _peer)) = tokio::try_join!(connect, accept)?;
        client.write_all(DUAL_STACK_PROBE).await?;
        let mut received = [0_u8; DUAL_STACK_PROBE.len()];
        accepted.read_exact(&mut received).await?;
        Ok::<bool, io::Error>(received == DUAL_STACK_PROBE)
    };
    matches!(
        timeout(DUAL_STACK_PROBE_DEADLINE, probe).await,
        Ok(Ok(true))
    )
}

fn ipv6_is_unsupported(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::Unsupported {
        return true;
    }
    matches!(
        error.raw_os_error(),
        // Linux, Darwin/BSD, and Winsock EPROTONOSUPPORT/EAFNOSUPPORT.
        Some(93 | 97 | 43 | 47 | 10043 | 10047)
    )
}

type McpHttpService = StreamableHttpService<McpHandler, LocalSessionManager>;

async fn run(
    listeners: Vec<TcpListener>,
    backend: Arc<dyn McpBackend>,
    transport_capabilities: Arc<McpTransportCapabilities>,
    cancellation: CancellationToken,
) {
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .with_sse_retry(None)
        .with_cancellation_token(cancellation.child_token())
        .with_max_request_body_bytes(MAX_REQUEST_BYTES)
        .with_stateless_protocol_metadata_required(true)
        .disable_allowed_hosts()
        .disable_allowed_origins();
    let handler = McpHandler::new(backend, transport_capabilities);
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let connection_limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let request_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));
    let mut acceptors = JoinSet::new();
    for listener in listeners {
        acceptors.spawn(accept_connections(
            listener,
            service.clone(),
            Arc::clone(&connection_limit),
            Arc::clone(&request_limit),
            cancellation.child_token(),
        ));
    }

    tokio::select! {
        () = cancellation.cancelled() => {}
        result = acceptors.join_next() => match result {
            Some(Ok(Err(error))) => tracing::error!(%error, "MCP accept failed"),
            Some(Err(error)) => tracing::error!(%error, "MCP accept task failed"),
            Some(Ok(Ok(()))) | None => {}
        }
    }
    cancellation.cancel();
    acceptors.abort_all();
    while acceptors.join_next().await.is_some() {}
}

async fn accept_connections(
    listener: TcpListener,
    service: McpHttpService,
    connection_limit: Arc<Semaphore>,
    request_limit: Arc<Semaphore>,
    cancellation: CancellationToken,
) -> io::Result<()> {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let Ok(connection_permit) = Arc::clone(&connection_limit).try_acquire_owned() else {
                    tracing::warn!(%peer, "MCP connection limit reached");
                    continue;
                };
                let service = service.clone();
                let request_limit = Arc::clone(&request_limit);
                connections.spawn(async move {
                    let _connection_permit = connection_permit;
                    let http = service_fn(move |request| {
                        serve(request, service.clone(), Arc::clone(&request_limit))
                    });
                    if let Err(error) = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), http)
                        .await
                    {
                        tracing::debug!(%error, "MCP connection closed");
                    }
                });
            }
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = result {
                    tracing::debug!(%error, "MCP connection task failed");
                }
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
    Ok(())
}

async fn serve(
    request: Request<Incoming>,
    mut service: McpHttpService,
    request_limit: Arc<Semaphore>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let Ok(_request_permit) = request_limit.try_acquire_owned() else {
        return Ok(transport_error_response(
            TransportError::RequestLimitReached,
        ));
    };
    if request.uri().path() != "/mcp" {
        return Ok(transport_error_response(TransportError::PathNotFound));
    }
    if request.method() != Method::POST {
        return Ok(transport_error_response(TransportError::MethodNotAllowed));
    }
    let (parts, body) = request.into_parts();
    let collected = match Limited::new(body, MAX_REQUEST_BYTES).collect().await {
        Ok(collected) => collected,
        Err(error) if error.downcast_ref::<LengthLimitError>().is_some() => {
            return Ok(transport_error_response(TransportError::BodyTooLarge));
        }
        Err(_) => return Ok(transport_error_response(TransportError::HttpMalformed)),
    };
    let body = collected.to_bytes();
    if serde_json::from_slice::<serde_json::Value>(&body).is_err() {
        return Ok(transport_error_response(TransportError::HttpMalformed));
    }
    let request = Request::from_parts(parts, Full::new(body));
    let response = match timeout(TRANSPORT_REQUEST_DEADLINE, service.call(request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(never)) => match never {},
        Err(_) => {
            return Ok(transport_error_response(
                TransportError::RequestDeadlineExceeded,
            ));
        }
    };
    let (parts, body) = response.into_parts();
    let Ok(collected) = Limited::new(body, MAX_RESPONSE_BYTES).collect().await else {
        return Ok(transport_error_response(TransportError::ResponseTooLarge));
    };
    if !parts.status.is_success() {
        return Ok(transport_error_response(TransportError::ProtocolInvalid));
    }
    Ok(Response::from_parts(parts, Full::new(collected.to_bytes())))
}

#[cfg(test)]
pub(super) async fn start_test_server(backend: Arc<dyn McpBackend>) -> io::Result<McpServer> {
    McpServer::start_on("127.0.0.1:0", backend).await
}
