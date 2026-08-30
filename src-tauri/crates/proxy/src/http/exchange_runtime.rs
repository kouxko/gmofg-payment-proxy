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
    Error as ExchangeError, Exchange, ExchangeId, Http, HttpRead, Pipeline, ProtocolExchange,
    ServerSlot, Write,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use super::{
    Clock, ConnectionContext, ErrorCode, FaultAction, ForwardRequest, HttpConnectionIdentity,
    HttpDirectionCapabilities, HttpProtocolCapabilityFactory, InformationalResponseSink, Message,
    PipelinePorts, ProxyError, ResponseDisposition, Result, UpstreamConnector,
};
use crate::listener::{ChildTaskError, ConnectionTaskScope};

mod endpoints;

use endpoints::{BufferedApp, BufferedHttpServer};

/// Exchange 完成后交回 HTTP wire 层的结果。
pub(crate) struct HttpExchangeOutput {
    pub(crate) informational_heads: Vec<Bytes>,
    pub(crate) disposition: ResponseDisposition,
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
    request: Option<Message>,
    response: Option<Message>,
    informational_heads: Vec<Bytes>,
    disposition: Option<ResponseDisposition>,
    mocked: bool,
    close_requested: bool,
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

    fn begin(&mut self, command: HttpExchangeCommand) {
        debug_assert!(
            self.current.is_none(),
            "HTTP transactions are strictly paired"
        );
        let close_requested = requests_connection_close(&command.request.message);
        self.current = Some(HttpTransaction {
            method: command.request.method,
            uri: command.request.uri,
            request: Some(command.request.message),
            response: None,
            informational_heads: Vec::new(),
            disposition: None,
            mocked: false,
            close_requested,
            completed: Some(command.completed),
        });
    }

    fn complete(&mut self) {
        let Some(mut transaction) = self.current.take() else {
            return;
        };
        let result = transaction.disposition.take().map_or_else(
            || {
                Err(ProxyError::new(
                    ErrorCode::Internal,
                    "HTTP Exchange completed without a downstream disposition",
                ))
            },
            |disposition| {
                Ok(HttpExchangeOutput {
                    informational_heads: std::mem::take(&mut transaction.informational_heads),
                    disposition,
                })
            },
        );
        if let Some(completed) = transaction.completed.take() {
            let _ = completed.send(result);
        }
    }

    fn fail(&mut self, error: &ProxyError) {
        let Some(mut transaction) = self.current.take() else {
            return;
        };
        if let Some(completed) = transaction.completed.take() {
            let _ = completed.send(Err(copy_error(error)));
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
        );
        let capabilities = self.capabilities;
        task_scope
            .spawn_owned(async move {
                let result = Exchange::<Http>::protocol_with(exchange_id, move || {
                    let upstream_capabilities = capabilities.create_upstream(identity.clone())?;
                    let downstream_capabilities = capabilities.create_downstream(identity)?;
                    Ok(ProtocolExchange::new(
                        app,
                        ServerSlot::new(Box::new(server)),
                        pipeline(upstream_capabilities),
                        pipeline(downstream_capabilities),
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
            let _ = sender
                .send(HttpExchangeInput::Fail(copy_error(error)))
                .await;
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
    ProxyError::new(ErrorCode::Io, "HTTP connection Exchange is closed")
}

fn copy_error(error: &ProxyError) -> ProxyError {
    ProxyError {
        code: error.code,
        message: error.message.clone(),
        external_package_call: error.external_package_call.clone(),
    }
}

fn pipeline<D: intercept_proxy_exchange::Direction>(
    capabilities: HttpDirectionCapabilities<D>,
) -> Pipeline<Http, D> {
    Pipeline::new(
        Box::new(HttpRead::new(capabilities.decode, capabilities.display)),
        Box::new(Write::new(capabilities.rules, capabilities.encode)),
    )
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
    let ExchangeError {
        message,
        external_package_call,
    } = error;
    let (code, detail) = message.split_once('\n').map_or(
        (crate::ErrorCode::Internal, message.as_str()),
        |(code, message)| {
            (
                crate::ErrorCode::from_stable_str(code).unwrap_or(crate::ErrorCode::Internal),
                message,
            )
        },
    );
    ProxyError::new(code, format!("HTTP Exchange failed: {detail}"))
        .with_external_package_call(external_package_call)
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
mod tests {
    use intercept_proxy_exchange::{
        Error, ExternalPackageCallFailure, ExternalPackageCallStage, ProtocolDirection,
        ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    };

    use super::exchange_error;
    use crate::ErrorCode;

    #[test]
    fn exchange_error_preserves_adapter_error_code() {
        let error = exchange_error(Error::new("UPSTREAM_READ_TIMEOUT\nserver reply timed out"));

        assert_eq!(error.code, ErrorCode::UpstreamReadTimeout.as_str());
        assert!(error.message.contains("server reply timed out"));
    }

    #[test]
    fn exchange_error_maps_core_failure_to_internal() {
        let error = exchange_error(Error::new("Server disconnected before replying"));

        assert_eq!(error.code, ErrorCode::Internal.as_str());
        assert!(
            error
                .message
                .contains("Server disconnected before replying")
        );
    }

    #[test]
    fn exchange_error_preserves_typed_external_package_failure() {
        let failure = ExternalPackageCallFailure {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("phase10.http").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            direction: ProtocolDirection::Upstream,
            stage: ExternalPackageCallStage::Display,
            method: "document.upstream.display".into(),
            request_id: Some("proxy-4".into()),
            remote_code: Some(-32412),
            stable_code: Some("DISPLAY_FAILED".into()),
            remote_message: Some("display rejected".into()),
            remote_data_summary: Some("object(fields=1)".into()),
        };
        let error = exchange_error(
            Error::new("EXTERNAL_PACKAGE_CALL_FAILED\ndisplay rejected")
                .with_external_package_call(failure.clone()),
        );
        assert_eq!(error.code, "EXTERNAL_PACKAGE_CALL_FAILED");
        assert_ne!(error.code, ErrorCode::Internal.as_str());
        assert_eq!(error.external_package_call.as_deref(), Some(&failure));
    }
}
