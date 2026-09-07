//! 缓冲 HTTP request/response 的 App Connection 与固定 Server Endpoint。
//!
//! Hyper 已完成 HTTP framing，因此这里每次 Reader 返回一个完整 `HttpContext`。App
//! Connection 的 upstream Reader 只产生当前请求一次；Server Connection 的 downstream
//! Reader 只产生当前回复一次。Writer 必须完成整条内存消息交接才返回成功。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Connection, Downstream, Error, Http, HttpContext, LocalHttpServer, Reader, Server,
    ServerConnection, Upstream, WriteOutcome, Writer,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
use super::HttpExchangeCommand;
use super::{
    Clock, ConnectionContext, FaultAction, ForwardRequest, HttpExchangeInput, HttpExchangeState,
    InformationalResponseSink, Message, PipelinePorts, ProxyError, ResponseDisposition,
    UpstreamConnector,
};
use crate::http::UpstreamExchange;
use crate::listener::{ChildTaskError, ConnectionTaskScope};

mod app;
pub(super) use app::BufferedApp;
#[cfg(test)]
use app::BufferedAppWriter;

#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalHttpServerConnector;

#[async_trait]
impl UpstreamConnector for LocalHttpServerConnector {
    async fn send(
        &self,
        _connection_context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        request: ForwardRequest,
        actions: &[FaultAction],
        write_receipt: super::super::UpstreamWriteReceipt,
        _informational: Option<&InformationalResponseSink>,
        cancellation: &CancellationToken,
    ) -> crate::Result<UpstreamExchange> {
        if cancellation.is_cancelled() {
            return Err(ProxyError::new(
                crate::ErrorCode::ProxyStopped,
                "proxy stopped before LocalHttpServer reply",
            ));
        }
        if actions.iter().any(|action| {
            !matches!(
                action,
                FaultAction::Delay(_) | FaultAction::MockResponse { .. }
            )
        }) {
            return Err(ProxyError::new(
                crate::ErrorCode::ConfigInvalid,
                "request-stage upstream transport fault is not applicable to LocalHttpServer",
            ));
        }

        let original = message_context(&request.message);
        let mut server = LocalHttpServer::new();
        let mut connection = server
            .connect(&original)
            .await
            .map_err(local_server_error)?;
        connection
            .writer()
            .write(original.clone())
            .await
            .map_err(local_server_error)?;
        write_receipt.confirm_flushed();
        let echoed = connection
            .reader()
            .read()
            .await
            .map_err(local_server_error)?
            .ok_or_else(|| {
                ProxyError::new(
                    crate::ErrorCode::Internal,
                    "LocalHttpServer closed before replying",
                )
            })?;
        connection.shutdown().await.map_err(local_server_error)?;

        let mut effective = request.message;
        super::super::capabilities::apply_http_context_changes(&mut effective, &original, &echoed)
            .map_err(|error| {
                let message = error.message.clone();
                ProxyError::from_exchange_error(crate::ErrorCode::Internal, message, error)
            })?;
        let mut response = Message::response(
            http::StatusCode::OK,
            &http::HeaderMap::new(),
            effective.body,
        );
        response.headers = effective
            .headers
            .into_iter()
            .filter(|header| {
                header.name.eq_ignore_ascii_case(b"content-type")
                    || header.name.eq_ignore_ascii_case(b"content-encoding")
            })
            .collect();
        response.set_content_length(response.body.len());
        Ok(response.into())
    }
}

fn local_server_error(error: Error) -> ProxyError {
    let message = error.message.clone();
    ProxyError::from_exchange_error(crate::ErrorCode::Internal, message, error)
}

pub(super) struct BufferedHttpServer {
    state: Arc<Mutex<HttpExchangeState>>,
    context: ConnectionContext,
    ports: Arc<dyn PipelinePorts>,
    upstream: Arc<dyn UpstreamConnector>,
    clock: Arc<dyn Clock>,
    cancellation: CancellationToken,
    informational: Option<InformationalResponseSink>,
    task_scope: ConnectionTaskScope,
}

impl BufferedHttpServer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        state: Arc<Mutex<HttpExchangeState>>,
        context: ConnectionContext,
        ports: Arc<dyn PipelinePorts>,
        upstream: Arc<dyn UpstreamConnector>,
        clock: Arc<dyn Clock>,
        cancellation: CancellationToken,
        informational: Option<InformationalResponseSink>,
        task_scope: ConnectionTaskScope,
    ) -> Self {
        Self {
            state,
            context,
            ports,
            upstream,
            clock,
            cancellation,
            informational,
            task_scope,
        }
    }
}

#[async_trait]
impl Server<Http> for BufferedHttpServer {
    async fn connect(
        &mut self,
        _first: &HttpContext,
    ) -> Result<Box<ServerConnection<Http>>, Error> {
        let response = Arc::new(Mutex::new(None));
        Ok(Box::new(BufferedServerConnection {
            reader: BufferedServerReader {
                state: Arc::clone(&self.state),
                response: Arc::clone(&response),
            },
            writer: BufferedServerWriter {
                state: Arc::clone(&self.state),
                context: self.context.clone(),
                ports: Arc::clone(&self.ports),
                upstream: Arc::clone(&self.upstream),
                clock: Arc::clone(&self.clock),
                cancellation: self.cancellation.clone(),
                informational: self.informational.clone(),
                task_scope: self.task_scope.clone(),
                response,
            },
        }))
    }
}

struct BufferedServerConnection {
    reader: BufferedServerReader,
    writer: BufferedServerWriter,
}

#[async_trait]
impl Connection<Http, Downstream, Upstream> for BufferedServerConnection {
    fn reader(&mut self) -> &mut dyn Reader<Http, Downstream> {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut dyn Writer<Http, Upstream> {
        &mut self.writer
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

struct BufferedServerReader {
    state: Arc<Mutex<HttpExchangeState>>,
    response: Arc<Mutex<Option<oneshot::Receiver<crate::Result<UpstreamExchange>>>>>,
}

#[async_trait]
impl Reader<Http, Downstream> for BufferedServerReader {
    async fn read(&mut self) -> Result<Option<HttpContext>, Error> {
        if let Some(response) = self
            .state
            .lock()
            .expect("HTTP Exchange state mutex poisoned")
            .current
            .as_ref()
            .and_then(|transaction| transaction.response.as_ref())
            .map(message_context)
        {
            return Ok(Some(response));
        }
        let receiver = self
            .response
            .lock()
            .expect("HTTP upstream response mutex poisoned")
            .take()
            .ok_or_else(|| {
                adapter_error(
                    "HTTP_RESPONSE_MISSING",
                    "HTTP upstream response transaction missing",
                )
            })?;
        let exchange = receiver
            .await
            .map_err(|_| {
                adapter_error(
                    "UPSTREAM_RESPONSE_DROPPED",
                    "upstream response task ended without a result",
                )
            })?
            .map_err(proxy_write_error)?;
        let mut state = self
            .state
            .lock()
            .expect("HTTP Exchange state mutex poisoned");
        let transaction = state
            .current
            .as_mut()
            .ok_or_else(|| adapter_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing"))?;
        if transaction.drop_response_after_read {
            return Err(write_error(
                crate::ErrorCode::ClientDisconnected.as_str(),
                "upstream response intentionally dropped after complete read",
            ));
        }
        let context = message_context(&exchange.final_response);
        transaction.informational_heads = exchange.informational_heads;
        transaction.response = Some(exchange.final_response);
        Ok(Some(context))
    }
}

struct BufferedServerWriter {
    state: Arc<Mutex<HttpExchangeState>>,
    context: ConnectionContext,
    ports: Arc<dyn PipelinePorts>,
    upstream: Arc<dyn UpstreamConnector>,
    clock: Arc<dyn Clock>,
    cancellation: CancellationToken,
    informational: Option<InformationalResponseSink>,
    task_scope: ConnectionTaskScope,
    response: Arc<Mutex<Option<oneshot::Receiver<crate::Result<UpstreamExchange>>>>>,
}

#[async_trait]
impl Writer<Http, Upstream> for BufferedServerWriter {
    async fn write(&mut self, _context: HttpContext) -> Result<WriteOutcome<HttpContext>, Error> {
        let (method, uri, request, actions) = {
            let mut state = self
                .state
                .lock()
                .expect("HTTP Exchange state mutex poisoned");
            let transaction = state.current.as_mut().ok_or_else(|| {
                write_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing")
            })?;
            let request = transaction.request.take().ok_or_else(|| {
                write_error(
                    "HTTP_REQUEST_MISSING",
                    "HTTP request missing before Server write",
                )
            })?;
            let actions = transaction.request_actions.take().ok_or_else(|| {
                write_error(
                    "HTTP_REQUEST_NOT_PREPARED",
                    "HTTP request actions missing before Server write",
                )
            })?;
            (
                transaction.method.clone(),
                transaction.uri.clone(),
                request,
                actions,
            )
        };
        let written = message_context(&request);
        if self.apply_request_actions(&actions).await? {
            return Ok(WriteOutcome::ReadReady(written));
        }
        Ok(
            match self
                .start_upstream_send(method, uri, request, actions)
                .await?
            {
                WriteOutcome::Committed(()) => WriteOutcome::Committed(written),
                WriteOutcome::ReadReady(()) => WriteOutcome::ReadReady(written),
            },
        )
    }
}

impl BufferedServerWriter {
    async fn start_upstream_send(
        &self,
        method: http::Method,
        uri: http::Uri,
        request: Message,
        actions: Vec<FaultAction>,
    ) -> Result<WriteOutcome<()>, Error> {
        let drop_response_after_read = actions.iter().any(|action| {
            matches!(
                action,
                FaultAction::DropResponse {
                    read_upstream: true
                }
            )
        });
        {
            let mut state = self
                .state
                .lock()
                .expect("HTTP Exchange state mutex poisoned");
            let transaction = state.current.as_mut().ok_or_else(|| {
                write_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing")
            })?;
            transaction.drop_response_after_read = drop_response_after_read;
        }
        let (write_receipt, write_completion) = super::super::upstream_write_receipt();
        let (response_sender, response_receiver) = oneshot::channel();
        *self
            .response
            .lock()
            .expect("HTTP upstream response mutex poisoned") = Some(response_receiver);
        let context = self.context.clone();
        let ports = Arc::clone(&self.ports);
        let upstream = Arc::clone(&self.upstream);
        let informational = self.informational.clone();
        let cancellation = self.cancellation.clone();
        let task_receipt = write_receipt.clone();
        self.task_scope
            .spawn_owned(async move {
                let result = tokio::select! {
                    biased;
                    () = cancellation.cancelled() => Err(ProxyError::new(
                        crate::ErrorCode::ProxyStopped,
                        "proxy stopped while sending upstream HTTP request",
                    )),
                    result = upstream.send(
                        &context,
                        ports.as_ref(),
                        ForwardRequest {
                            method,
                            uri,
                            message: request,
                        },
                        &actions,
                        task_receipt.clone(),
                        informational.as_ref(),
                        &cancellation,
                    ) => result,
                };
                match &result {
                    Err(error) => task_receipt.fail(error),
                    Ok(_) if task_receipt.is_pending() => task_receipt.response_ready(),
                    Ok(_) => {}
                }
                let _ = response_sender.send(result);
                Ok::<(), ChildTaskError>(())
            })
            .map_err(|_| {
                write_error(
                    crate::ErrorCode::Internal.as_str(),
                    "connection task scope closed before upstream HTTP send",
                )
            })?;
        drop(write_receipt);
        write_completion
            .wait(&self.cancellation)
            .await
            .map_err(proxy_write_error)
    }

    async fn apply_request_actions(&self, actions: &[FaultAction]) -> Result<bool, Error> {
        for action in actions {
            match action {
                FaultAction::Delay(duration) => {
                    tokio::select! {
                        () = self.cancellation.cancelled() => return Err(write_error(
                            crate::ErrorCode::ProxyStopped.as_str(),
                            "proxy stopped during request delay",
                        )),
                        () = self.clock.sleep(*duration) => {}
                    }
                }
                FaultAction::DisconnectBeforeUpstream => {
                    return Err(write_error(
                        crate::ErrorCode::ClientDisconnected.as_str(),
                        "request intentionally disconnected before upstream",
                    ));
                }
                FaultAction::MockResponse {
                    status,
                    headers,
                    body,
                } => {
                    let response = crate::fault::mock_response(*status, headers, body.clone());
                    let mut state = self
                        .state
                        .lock()
                        .expect("HTTP Exchange state mutex poisoned");
                    let transaction = state.current.as_mut().ok_or_else(|| {
                        write_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing")
                    })?;
                    transaction.response = Some(response);
                    transaction.mocked = true;
                    return Ok(true);
                }
                FaultAction::RejectTls => {
                    return Err(write_error(
                        crate::ErrorCode::TlsHandshakeFailed.as_str(),
                        "TLS intentionally rejected",
                    ));
                }
                _ => {}
            }
        }
        Ok(false)
    }
}

pub(super) fn message_context(message: &Message) -> HttpContext {
    HttpContext {
        header: message_header_text(message),
        body: String::from_utf8_lossy(&message.body).into_owned(),
        body_is_utf8: std::str::from_utf8(&message.body).is_ok(),
        wire_body: message.body.to_vec(),
    }
}

fn message_header_text(message: &Message) -> String {
    let bytes = message.reconstruct();
    let header_len = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map_or(bytes.len(), |position| position + 4);
    String::from_utf8_lossy(&bytes[..header_len]).into_owned()
}

fn adapter_error(code: impl AsRef<str>, message: impl AsRef<str>) -> Error {
    Error::coded(code.as_ref(), message.as_ref())
}

fn write_error(code: impl AsRef<str>, message: impl AsRef<str>) -> Error {
    // Writer 错误由上层 `Pipeline::write` 统一附带 direction/context 记录一次。
    // Endpoint 只返回错误，避免 UI 时间线出现同一失败的重复记录。
    adapter_error(code, message)
}

fn proxy_write_error(error: super::ProxyError) -> Error {
    error.into_exchange_error()
}

#[cfg(test)]
mod tests;
