//! 缓冲 HTTP request/response 的 App Connection 与固定 Server Endpoint。
//!
//! Hyper 已完成 HTTP framing，因此这里每次 Reader 返回一个完整 `HttpContext`。App
//! Connection 的 upstream Reader 只产生当前请求一次；Server Connection 的 downstream
//! Reader 只产生当前回复一次。Writer 必须完成整条内存消息交接才返回成功。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::Bytes;
use intercept_proxy_exchange::{
    Connection, Downstream, Error, Http, HttpContext, Reader, Server, ServerConnection, Upstream,
    Writer,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
use super::HttpExchangeCommand;
use super::{
    Clock, ConnectionContext, FaultAction, ForwardRequest, HttpExchangeInput, HttpExchangeState,
    InformationalResponseSink, Message, PipelinePorts, ResponseDisposition, UpstreamConnector,
};
use crate::fault::project_response_for_observation;

pub(super) struct BufferedApp {
    reader: BufferedAppReader,
    writer: BufferedAppWriter,
}

impl BufferedApp {
    pub(super) fn new(
        state: Arc<Mutex<HttpExchangeState>>,
        receiver: mpsc::Receiver<HttpExchangeInput>,
        endpoint: String,
    ) -> Self {
        Self {
            reader: BufferedAppReader {
                state: Arc::clone(&state),
                receiver,
                endpoint,
            },
            writer: BufferedAppWriter { state },
        }
    }
}

#[async_trait]
impl Connection<Http, Upstream, Downstream> for BufferedApp {
    fn reader(&mut self) -> &mut dyn Reader<Http, Upstream> {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut dyn Writer<Http, Downstream> {
        &mut self.writer
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

struct BufferedAppReader {
    state: Arc<Mutex<HttpExchangeState>>,
    receiver: mpsc::Receiver<HttpExchangeInput>,
    endpoint: String,
}

#[async_trait]
impl Reader<Http, Upstream> for BufferedAppReader {
    async fn read(&mut self) -> Result<Option<HttpContext>, Error> {
        let Some(input) = self.receiver.recv().await else {
            return Ok(None);
        };
        let command = match input {
            HttpExchangeInput::Request(command) => command,
            HttpExchangeInput::Fail(error) => {
                return Err(adapter_error(error.code, error.message));
            }
        };
        if command.endpoint != self.endpoint {
            let error = adapter_error(
                crate::ErrorCode::ConfigInvalid.as_str(),
                format!(
                    "HTTP connection Endpoint changed from {} to {}",
                    self.endpoint, command.endpoint
                ),
            );
            let _ = command
                .completed
                .send(Err(super::exchange_error(error.clone())));
            return Err(error);
        }
        self.state
            .lock()
            .expect("HTTP Exchange state mutex poisoned")
            .begin(command);
        let state = self
            .state
            .lock()
            .expect("HTTP Exchange state mutex poisoned");
        let transaction = state
            .current
            .as_ref()
            .ok_or_else(|| adapter_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing"))?;
        let request = transaction
            .request
            .as_ref()
            .ok_or_else(|| adapter_error("HTTP_REQUEST_MISSING", "HTTP request missing"))?;
        Ok(Some(message_context(request)))
    }
}

struct BufferedAppWriter {
    state: Arc<Mutex<HttpExchangeState>>,
}

#[async_trait]
impl Writer<Http, Downstream> for BufferedAppWriter {
    async fn write(&mut self, context: HttpContext) -> Result<HttpContext, Error> {
        let (mut response, mocked, close_requested, pipeline_context, ports, cancellation) = {
            let mut state = self
                .state
                .lock()
                .expect("HTTP Exchange state mutex poisoned");
            let transaction = state.current.as_mut().ok_or_else(|| {
                write_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing")
            })?;
            let response = transaction.response.take().ok_or_else(|| {
                write_error(
                    "HTTP_RESPONSE_MISSING",
                    "HTTP response missing before App write",
                )
            })?;
            (
                response,
                transaction.mocked,
                transaction.close_requested,
                state.context.clone(),
                Arc::clone(&state.ports),
                state.cancellation.clone(),
            )
        };
        let original = message_context(&response);
        let actions = if mocked {
            Vec::new()
        } else {
            ports
                .apply_response_policy(&pipeline_context, &mut response)
                .await
                .map_err(proxy_write_error)?
        };
        apply_context_changes(&mut response, &original, &context)?;
        if close_requested {
            response.remove_header("connection");
            response
                .headers
                .push(crate::message::RawHeader::new("Connection", "close"));
        }
        let written = project_response_for_observation(response.clone(), &actions)
            .map_err(proxy_write_error)?
            .map_or_else(
                || message_context(&response),
                |message| message_context(&message),
            );
        let disposition = if mocked {
            ResponseDisposition::Send {
                message: response,
                schedule: crate::traffic::TrafficSchedule::default(),
            }
        } else {
            crate::fault::apply_response_actions(response, &actions, &cancellation)
                .await
                .map_err(proxy_write_error)?
        };
        let mut state = self
            .state
            .lock()
            .expect("HTTP Exchange state mutex poisoned");
        state
            .current
            .as_mut()
            .ok_or_else(|| write_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing"))?
            .disposition = Some(disposition);
        state.complete();
        Ok(written)
    }
}

pub(super) struct BufferedHttpServer {
    state: Arc<Mutex<HttpExchangeState>>,
    context: ConnectionContext,
    ports: Arc<dyn PipelinePorts>,
    upstream: Arc<dyn UpstreamConnector>,
    clock: Arc<dyn Clock>,
    cancellation: CancellationToken,
    informational: Option<InformationalResponseSink>,
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
    ) -> Self {
        Self {
            state,
            context,
            ports,
            upstream,
            clock,
            cancellation,
            informational,
        }
    }
}

#[async_trait]
impl Server<Http> for BufferedHttpServer {
    async fn connect(
        &mut self,
        _first: &HttpContext,
    ) -> Result<Box<ServerConnection<Http>>, Error> {
        Ok(Box::new(BufferedServerConnection {
            reader: BufferedServerReader {
                state: Arc::clone(&self.state),
            },
            writer: BufferedServerWriter {
                state: Arc::clone(&self.state),
                context: self.context.clone(),
                ports: Arc::clone(&self.ports),
                upstream: Arc::clone(&self.upstream),
                clock: Arc::clone(&self.clock),
                cancellation: self.cancellation.clone(),
                informational: self.informational.clone(),
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
}

#[async_trait]
impl Reader<Http, Downstream> for BufferedServerReader {
    async fn read(&mut self) -> Result<Option<HttpContext>, Error> {
        let state = self
            .state
            .lock()
            .expect("HTTP Exchange state mutex poisoned");
        let transaction = state
            .current
            .as_ref()
            .ok_or_else(|| adapter_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing"))?;
        let response = transaction
            .response
            .as_ref()
            .ok_or_else(|| adapter_error("HTTP_RESPONSE_MISSING", "HTTP response missing"))?;
        Ok(Some(message_context(response)))
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
}

#[async_trait]
impl Writer<Http, Upstream> for BufferedServerWriter {
    async fn write(&mut self, context: HttpContext) -> Result<HttpContext, Error> {
        let (method, uri, mut request) = {
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
            (transaction.method.clone(), transaction.uri.clone(), request)
        };
        let original = message_context(&request);
        let actions = self
            .ports
            .apply_request_policy(&self.context, &mut request)
            .await
            .map_err(proxy_write_error)?;
        apply_context_changes(&mut request, &original, &context)?;
        let written = message_context(&request);
        if self.apply_request_actions(&actions).await? {
            return Ok(written);
        }
        let exchange = self
            .upstream
            .send(
                &self.context,
                self.ports.as_ref(),
                ForwardRequest {
                    method,
                    uri,
                    message: request,
                },
                &actions,
                self.informational.as_ref(),
                &self.cancellation,
            )
            .await
            .map_err(proxy_write_error)?;
        if actions.iter().any(|action| {
            matches!(
                action,
                FaultAction::DropResponse {
                    read_upstream: true
                }
            )
        }) {
            return Err(write_error(
                crate::ErrorCode::ClientDisconnected.as_str(),
                "upstream response intentionally dropped after complete read",
            ));
        }
        let mut state = self
            .state
            .lock()
            .expect("HTTP Exchange state mutex poisoned");
        let transaction = state
            .current
            .as_mut()
            .ok_or_else(|| write_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing"))?;
        transaction.informational_heads = exchange.informational_heads;
        transaction.response = Some(exchange.final_response);
        Ok(written)
    }
}

impl BufferedServerWriter {
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

fn message_context(message: &Message) -> HttpContext {
    HttpContext {
        header: message_header_text(message),
        body: String::from_utf8_lossy(&message.body).into_owned(),
        body_is_utf8: std::str::from_utf8(&message.body).is_ok(),
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

fn apply_context_changes(
    message: &mut Message,
    original: &HttpContext,
    encoded: &HttpContext,
) -> Result<(), Error> {
    // Wire policy 先作用于权威 Message，协议 Encode 只覆盖自己确实改动的部分。这样既保持
    // 原有动作顺序，又让 Reader/Display 固定为接收时事实。
    if encoded.header != original.header {
        let parsed = Message::from_raw_http1_head(
            encoded.header.as_bytes(),
            Bytes::copy_from_slice(encoded.body.as_bytes()),
        )
        .map_err(|error| write_error(error.code, error.message))?;
        message.start_line = parsed.start_line;
        message.headers = parsed.headers;
    }
    // 非 UTF-8 body 的文本 Context 是 lossy view；未改变时保留权威原字节和 wire policy。
    if encoded.body != original.body {
        message.replace_body(Bytes::copy_from_slice(encoded.body.as_bytes()));
    }
    Ok(())
}

fn adapter_error(code: impl AsRef<str>, message: impl AsRef<str>) -> Error {
    Error::new(format!("{}\n{}", code.as_ref(), message.as_ref()))
}

fn write_error(code: impl AsRef<str>, message: impl AsRef<str>) -> Error {
    // Writer 错误由上层 `Pipeline::write` 统一附带 direction/context 记录一次。
    // Endpoint 只返回错误，避免 UI 时间线出现同一失败的重复记录。
    adapter_error(code, message)
}

fn proxy_write_error(error: super::ProxyError) -> Error {
    write_error(error.code, error.message)
}

#[cfg(test)]
mod tests;
