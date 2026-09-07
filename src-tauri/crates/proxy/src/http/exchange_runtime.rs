//! 已缓冲 HTTP 消息到 `Exchange<Http>` 的生产装配。
//!
//! Hyper 继续负责 HTTP/1 framing 和原始 head 捕获；进入本模块后，请求和响应与 Socket
//! 使用同一个连接级 Exchange 状态机。`PipelinePorts` 只在 Reader 产出 `HttpContext`
//! 之前执行产品级 wire policy；协议阶段只能来自 capability factory 装配的
//! Decode、Display、Rules、Encode。

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http::{Method, Uri};
use intercept_proxy_exchange::{
    Direction, DirectionKind, Envelope, Error as ExchangeError, ErrorStage, Exchange, ExchangeId,
    Http, HttpContext, HttpRead, Pipeline, PrepareWrite, ProtocolExchange, ServerSlot, Write,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::{
    Clock, ConnectionContext, ErrorCode, FaultAction, Finalize, ForwardRequest,
    HttpConnectionIdentity, HttpDirectionCapabilities, HttpMessagePrepare,
    HttpProtocolCapabilityFactory, InformationalResponseSink, Message, PipelinePorts, ProxyError,
    ResponseDisposition, Result, UpstreamConnector,
};
use crate::listener::{ChildTaskError, ConnectionTaskScope};

mod endpoints;
mod request_target;

pub(crate) use endpoints::LocalHttpServerConnector;
use endpoints::{BufferedApp, BufferedHttpServer};
use request_target::finalized_upstream_uri;

/// Exchange 完成后交回 HTTP wire 层的结果。
pub(crate) struct HttpExchangeOutput {
    pub(crate) informational_heads: Vec<Bytes>,
    pub(crate) disposition: ResponseDisposition,
    pub(crate) write_completed: oneshot::Sender<Result<()>>,
}

pub(super) struct HttpExchangeCommand {
    endpoint: String,
    request: HttpExchangeRequest,
    completed: oneshot::Sender<Result<HttpExchangeOutput>>,
}

/// Inputs accepted by the connection-owned Exchange actor.
///
/// Closing the channel means the App closed normally. A proxy-side request failure must be an
/// explicit input so the Exchange timeline cannot mistake it for a successful EOF.
pub(super) enum HttpExchangeInput {
    Request(HttpExchangeCommand),
    Fail(ProxyError),
}

/// 单笔 HTTP 交易的共享状态。
///
/// 每个阶段只在 await 前后短暂持锁，绝不跨 await 持有同步锁。所有权集中在这里是为了让
/// `Decode/Encode` 能力和 Endpoint Writer 共同维护完整 HTTP Message，而 Envelope 仍只暴露
/// 经协议包处理的 UTF-8 Body。
pub(super) struct HttpExchangeState {
    context: ConnectionContext,
    ports: Arc<dyn PipelinePorts>,
    cancellation: CancellationToken,
    current: Option<HttpTransaction>,
}

struct HttpTransaction {
    method: Method,
    uri: Uri,
    app_request_url: String,
    request: Option<Message>,
    response: Option<Message>,
    request_actions: Option<Vec<FaultAction>>,
    response_actions: Option<Vec<FaultAction>>,
    informational_heads: Vec<Bytes>,
    disposition: Option<ResponseDisposition>,
    mocked: bool,
    close_requested: bool,
    drop_response_after_read: bool,
    completed: Option<oneshot::Sender<Result<HttpExchangeOutput>>>,
}

impl HttpExchangeState {
    fn new(
        context: ConnectionContext,
        ports: Arc<dyn PipelinePorts>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            context,
            ports,
            cancellation,
            current: None,
        }
    }

    fn begin(&mut self, command: HttpExchangeCommand) -> Result<()> {
        debug_assert!(
            self.current.is_none(),
            "HTTP transactions are strictly paired"
        );
        let close_requested = requests_connection_close(&command.request.message);
        let app_request_url = command
            .request
            .message
            .start_line
            .split_ascii_whitespace()
            .nth(1)
            .map(str::to_owned)
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCode::Internal,
                    "HTTP request start-line has no request target",
                )
            })?;
        self.current = Some(HttpTransaction {
            method: command.request.method,
            uri: command.request.uri,
            app_request_url,
            request: Some(command.request.message),
            response: None,
            request_actions: None,
            response_actions: None,
            informational_heads: Vec::new(),
            disposition: None,
            mocked: false,
            close_requested,
            drop_response_after_read: false,
            completed: Some(command.completed),
        });
        Ok(())
    }

    fn complete(&mut self) -> Option<oneshot::Receiver<Result<()>>> {
        let mut transaction = self.current.take()?;
        let mut write_completion = None;
        let result = transaction.disposition.take().map_or_else(
            || {
                Err(ProxyError::new(
                    ErrorCode::Internal,
                    "HTTP Exchange completed without a downstream disposition",
                ))
            },
            |disposition| {
                let (write_completed, completed) = oneshot::channel();
                write_completion = Some(completed);
                Ok(HttpExchangeOutput {
                    informational_heads: std::mem::take(&mut transaction.informational_heads),
                    disposition,
                    write_completed,
                })
            },
        );
        if let Some(completed) = transaction.completed.take() {
            let _ = completed.send(result);
        }
        write_completion
    }

    fn fail(&mut self, error: &ProxyError) {
        let Some(mut transaction) = self.current.take() else {
            return;
        };
        if let Some(completed) = transaction.completed.take() {
            let _ = completed.send(Err(error.clone()));
        }
    }
}

fn requests_connection_close(message: &Message) -> bool {
    message.headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case(b"connection")
            && String::from_utf8_lossy(&header.value)
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("close"))
    })
}

pub(crate) struct HttpExchangeRequest {
    pub(crate) method: Method,
    pub(crate) uri: Uri,
    pub(crate) message: Message,
}

pub(crate) struct HttpExchangeRuntime {
    pub(crate) context: ConnectionContext,
    pub(crate) ports: Arc<dyn PipelinePorts>,
    pub(crate) upstream: Arc<dyn UpstreamConnector>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) cancellation: CancellationToken,
    pub(crate) informational: Option<InformationalResponseSink>,
    pub(crate) capabilities: Arc<dyn HttpProtocolCapabilityFactory>,
    pub(crate) endpoint: String,
}

/// 一条 accepted HTTP connection 独占的长期 Exchange 句柄。
pub(crate) struct HttpExchangeConnection {
    sender: std::sync::Mutex<Option<mpsc::Sender<HttpExchangeInput>>>,
}

impl HttpExchangeRuntime {
    pub(crate) fn start(self, task_scope: &ConnectionTaskScope) -> Result<HttpExchangeConnection> {
        let exchange_id = ExchangeId::new(self.context.connection_id.as_u128());
        let span = observation_span(self.capabilities.as_ref(), &self.context, &self.endpoint);
        let identity = HttpConnectionIdentity::from(&self.context);
        let state = Arc::new(Mutex::new(HttpExchangeState::new(
            self.context.clone(),
            Arc::clone(&self.ports),
            self.cancellation.clone(),
        )));
        let (sender, receiver) = mpsc::channel(1);
        let app = Box::new(BufferedApp::new(
            Arc::clone(&state),
            receiver,
            self.endpoint.clone(),
        ));
        let server = BufferedHttpServer::new(
            Arc::clone(&state),
            self.context,
            Arc::clone(&self.ports),
            self.upstream,
            self.clock,
            self.cancellation,
            self.informational,
            task_scope.clone(),
        );
        let capabilities = self.capabilities;
        task_scope
            .spawn_owned(async move {
                let pipeline_state = Arc::clone(&state);
                let result = Exchange::<Http>::protocol_with(exchange_id, move || {
                    let upstream_capabilities = capabilities
                        .create_upstream(identity.clone())
                        .map_err(|error| {
                            error.at(ErrorStage::CapabilityFactory, DirectionKind::Upstream)
                        })?;
                    let downstream_capabilities =
                        capabilities.create_downstream(identity).map_err(|error| {
                            error.at(ErrorStage::CapabilityFactory, DirectionKind::Downstream)
                        })?;
                    Ok(ProtocolExchange::new(
                        app,
                        ServerSlot::new(Box::new(server)),
                        pipeline(upstream_capabilities, Arc::clone(&pipeline_state)),
                        pipeline(downstream_capabilities, Arc::clone(&pipeline_state)),
                    ))
                })
                .instrument(span)
                .await
                .map_err(exchange_error);
                if let Err(error) = &result {
                    state
                        .lock()
                        .expect("HTTP Exchange state mutex poisoned")
                        .fail(error);
                }
                result.map_err(ChildTaskError::from_proxy)
            })
            .map_err(|_| {
                ProxyError::new(
                    ErrorCode::Internal,
                    "connection task scope closed before HTTP Exchange could start",
                )
            })?;
        Ok(HttpExchangeConnection {
            sender: std::sync::Mutex::new(Some(sender)),
        })
    }
}

impl HttpExchangeConnection {
    pub(crate) async fn exchange(
        &self,
        endpoint: String,
        request: HttpExchangeRequest,
    ) -> Result<HttpExchangeOutput> {
        let sender = self
            .sender
            .lock()
            .expect("HTTP Exchange sender mutex poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(exchange_closed)?;
        let (completed, response) = oneshot::channel();
        sender
            .send(HttpExchangeInput::Request(HttpExchangeCommand {
                endpoint,
                request,
                completed,
            }))
            .await
            .map_err(|_| exchange_closed())?;
        response.await.map_err(|_| exchange_closed())?
    }

    /// Terminates the actor with the concrete proxy-side failure.
    ///
    /// The sender is removed before `await`, so the synchronous mutex guard never crosses a
    /// suspension point and no later HTTP request can be paired with the failed connection.
    pub(crate) async fn fail(&self, error: &ProxyError) {
        let sender = self
            .sender
            .lock()
            .expect("HTTP Exchange sender mutex poisoned")
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(HttpExchangeInput::Fail(error.clone())).await;
        }
    }

    pub(crate) fn shutdown(&self) {
        self.sender
            .lock()
            .expect("HTTP Exchange sender mutex poisoned")
            .take();
    }
}

fn exchange_closed() -> ProxyError {
    let mut error = ProxyError::new(ErrorCode::Io, "HTTP connection Exchange is closed");
    error.exchange_code = Some("HTTP_EXCHANGE_CLOSED".into());
    error
}

fn pipeline<D: intercept_proxy_exchange::Direction>(
    capabilities: HttpDirectionCapabilities<D>,
    state: Arc<Mutex<HttpExchangeState>>,
) -> Pipeline<Http, D> {
    Pipeline::new(
        Box::new(HttpRead::new(capabilities.decode, capabilities.display)),
        Box::new(Write::prepared(Box::new(BufferedHttpPrepare {
            state,
            prepare: capabilities.prepare,
            finalize: capabilities.finalize,
        }))),
    )
}

struct BufferedHttpPrepare<D: Direction> {
    state: Arc<Mutex<HttpExchangeState>>,
    prepare: Box<dyn HttpMessagePrepare<D>>,
    finalize: Box<dyn Finalize>,
}

#[async_trait::async_trait]
impl<D: Direction> PrepareWrite<Http, D> for BufferedHttpPrepare<D> {
    async fn prepare(
        &mut self,
        envelope: &Envelope<Http, D>,
    ) -> std::result::Result<HttpContext, ExchangeError> {
        let (connection, ports, request, mut message, mocked) = {
            let state = self
                .state
                .lock()
                .expect("HTTP Exchange state mutex poisoned");
            let transaction = state.current.as_ref().ok_or_else(|| {
                ExchangeError::coded("HTTP_TRANSACTION_MISSING", "HTTP transaction missing")
            })?;
            let mut request = super::HttpRequestMetadata::from_method_and_uri(
                &transaction.method,
                &transaction.uri,
            )
            .map_err(super::capabilities::proxy_error)?;
            let message = match D::KIND {
                DirectionKind::Upstream => transaction.request.clone(),
                DirectionKind::Downstream => transaction.response.clone(),
            }
            .ok_or_else(|| {
                ExchangeError::coded(
                    "HTTP_MESSAGE_MISSING",
                    "HTTP transaction message missing before write preparation",
                )
            })?;
            request.url.clone_from(&transaction.app_request_url);
            (
                state.context.clone(),
                Arc::clone(&state.ports),
                request,
                message,
                transaction.mocked,
            )
        };
        let actions = if D::KIND == DirectionKind::Downstream && mocked {
            self.finalize
                .finalize(&request, &mut message)
                .await
                .map_err(|error| error.at(ErrorStage::Finalize, D::KIND))?;
            Vec::new()
        } else {
            self.prepare
                .prepare(
                    &connection,
                    &request,
                    &mut message,
                    envelope.context(),
                    envelope.document(),
                    ports.as_ref(),
                )
                .await?
        };
        let prepared = endpoints::message_context(&message);
        let mut state = self
            .state
            .lock()
            .expect("HTTP Exchange state mutex poisoned");
        let transaction = state.current.as_mut().ok_or_else(|| {
            ExchangeError::coded("HTTP_TRANSACTION_MISSING", "HTTP transaction missing")
        })?;
        match D::KIND {
            DirectionKind::Upstream => {
                transaction.uri = finalized_upstream_uri(&transaction.uri, &request, &message)?;
                transaction.request = Some(message);
                transaction.request_actions = Some(actions);
            }
            DirectionKind::Downstream => {
                transaction.response = Some(message);
                transaction.response_actions = Some(actions);
            }
        }
        Ok(prepared)
    }
}

fn observation_span(
    factory: &dyn HttpProtocolCapabilityFactory,
    context: &ConnectionContext,
    endpoint: &str,
) -> tracing::Span {
    let metadata = factory.observation_metadata();
    let runtime_epoch = context.runtime_epoch.to_string();
    let connection_id = context.connection_id.to_string();
    let peer = context.peer_addr.to_string();
    tracing::info_span!(
        target: "intercept_proxy::exchange::diagnostic",
        "http_exchange",
        workspace_id = metadata.workspace_id.as_str(),
        listener_id = metadata.listener_id.as_str(),
        runtime_epoch = runtime_epoch.as_str(),
        connection_id = connection_id.as_str(),
        peer = peer.as_str(),
        protocol = "http",
        endpoint,
    )
}

fn exchange_error(error: ExchangeError) -> ProxyError {
    let code = match error.code.as_deref() {
        Some("HTTP_ENDPOINT_CHANGED") => crate::ErrorCode::ConfigInvalid,
        code => code
            .and_then(crate::ErrorCode::from_stable_str)
            .unwrap_or(crate::ErrorCode::Internal),
    };
    let message = format!("HTTP Exchange failed: {}", error.message);
    ProxyError::from_exchange_error(code, message, error)
}

impl From<ForwardRequest> for HttpExchangeRequest {
    fn from(request: ForwardRequest) -> Self {
        Self {
            method: request.method,
            uri: request.uri,
            message: request.message,
        }
    }
}

#[cfg(test)]
mod tests;
