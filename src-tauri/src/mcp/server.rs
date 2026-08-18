//! Loopback-only, stateless Streamable HTTP transport backed by official `rmcp`.

use std::{convert::Infallible, fmt, io, net::SocketAddr, sync::Arc, time::Duration};

use bytes::Bytes;
use http_body_util::{BodyExt as _, Full, Limited};
use hyper::{
    Method, Request, Response, StatusCode, body::Incoming, header::CONTENT_TYPE,
    server::conn::http1, service::service_fn,
};
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio::{
    net::TcpListener,
    sync::Semaphore,
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tower_service::Service as _;

use super::{backend::ReadOnlyMcpBackend, protocol::ReadOnlyMcpHandler};

pub const MCP_ADDRESS: &str = "127.0.0.1:17653";
pub const MCP_ENDPOINT: &str = "http://127.0.0.1:17653/mcp";
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 16;
const MAX_CONCURRENT_REQUESTS: usize = 32;
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct ReadOnlyMcpServer {
    inner: Arc<ServerInner>,
}

struct ServerInner {
    local_addr: SocketAddr,
    cancellation: CancellationToken,
    task: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for ReadOnlyMcpServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadOnlyMcpServer")
            .field("local_addr", &self.inner.local_addr)
            .field("cancelled", &self.inner.cancellation.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl ReadOnlyMcpServer {
    pub async fn start(backend: Arc<dyn ReadOnlyMcpBackend>) -> io::Result<Self> {
        Self::start_on(MCP_ADDRESS, backend).await
    }

    async fn start_on(address: &str, backend: Arc<dyn ReadOnlyMcpBackend>) -> io::Result<Self> {
        let listener = TcpListener::bind(address).await?;
        let local_addr = listener.local_addr()?;
        if !local_addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "MCP server must bind a loopback address",
            ));
        }
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(run(listener, backend, cancellation.clone()));
        Ok(Self {
            inner: Arc::new(ServerInner {
                local_addr,
                cancellation,
                task: std::sync::Mutex::new(Some(task)),
            }),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.inner.local_addr
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
            tracing::warn!("read-only MCP shutdown exceeded bounded deadline; task aborted");
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

type McpHttpService = StreamableHttpService<ReadOnlyMcpHandler, LocalSessionManager>;

async fn run(
    listener: TcpListener,
    backend: Arc<dyn ReadOnlyMcpBackend>,
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
        .with_allowed_hosts(["127.0.0.1", "localhost"])
        .with_allowed_origins([
            "null",
            "http://127.0.0.1",
            "http://127.0.0.1:17653",
            "http://localhost",
            "http://localhost:17653",
        ]);
    let handler = ReadOnlyMcpHandler::new(backend);
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let connection_limit = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let request_limit = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) if peer.ip().is_loopback() => {
                    let Ok(connection_permit) = Arc::clone(&connection_limit).try_acquire_owned() else {
                        tracing::warn!(%peer, "read-only MCP connection limit reached");
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
                            tracing::debug!(%error, "read-only MCP connection closed");
                        }
                    });
                }
                Ok((_stream, peer)) => tracing::warn!(%peer, "rejected non-loopback MCP connection"),
                Err(error) => {
                    tracing::error!(%error, "read-only MCP accept failed");
                    break;
                }
            },
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = result {
                    tracing::debug!(%error, "read-only MCP connection task failed");
                }
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

async fn serve(
    request: Request<Incoming>,
    mut service: McpHttpService,
    request_limit: Arc<Semaphore>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let Ok(_request_permit) = request_limit.try_acquire_owned() else {
        return Ok(text_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "MCP request concurrency limit reached",
        ));
    };
    if request.uri().path() != "/mcp" {
        return Ok(text_response(StatusCode::NOT_FOUND, "Not Found"));
    }
    if request.method() != Method::POST {
        return Ok(text_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "POST required",
        ));
    }
    let response = match timeout(REQUEST_DEADLINE, service.call(request)).await {
        Ok(Ok(response)) => response,
        Ok(Err(never)) => match never {},
        Err(_) => {
            return Ok(text_response(
                StatusCode::GATEWAY_TIMEOUT,
                "MCP request deadline exceeded",
            ));
        }
    };
    let (parts, body) = response.into_parts();
    let Ok(collected) = Limited::new(body, MAX_RESPONSE_BYTES).collect().await else {
        return Ok(text_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "MCP response exceeds output budget",
        ));
    };
    Ok(Response::from_parts(parts, Full::new(collected.to_bytes())))
}

fn text_response(status: StatusCode, text: &'static str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(text.as_bytes())))
        .expect("static response is valid")
}

#[cfg(test)]
pub(super) async fn start_test_server(
    backend: Arc<dyn ReadOnlyMcpBackend>,
) -> io::Result<ReadOnlyMcpServer> {
    ReadOnlyMcpServer::start_on("127.0.0.1:0", backend).await
}
