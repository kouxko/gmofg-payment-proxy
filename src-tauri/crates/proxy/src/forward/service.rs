//! 标准 HTTP/1.1 正向代理运行时。
//!
//! 该模块不依赖 Tauri、产品配置或证书存储。Host 负责把经过领域校验的监听配置和认证
//! 适配器注入进来；运行时负责协议语义、目标连接、背压、超时和取消。
//! 当前 Exchange 架构明确拒绝 CONNECT 与 Upgrade，不建立 Exchange 外旁路隧道。

#[cfg(test)]
use std::convert::Infallible;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use http::header::{HOST, HeaderValue, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION};
use http::{Method, Request, Response, StatusCode, Uri};
#[cfg(test)]
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::client::conn::http1 as client_http1;
use hyper::server::conn::http1 as server_http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
#[cfg(test)]
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::http::{HttpProtocolCapabilityFactory, PipelinePorts, traffic_schedule};
use crate::listener::{ChildTaskError, ConnectionTaskScope};
use crate::message::MessageLimits;
use crate::reverse::{DownstreamTlsAcceptor, ReverseDownstreamTls};
use crate::supervisor::ChannelId;
use crate::traffic::TrafficDirection;
use crate::transport::{BoxIo, ConnectionContext};
use crate::{ErrorCode, ProxyError, Result};

#[path = "body.rs"]
mod body;
#[path = "config.rs"]
mod config;
#[path = "exchange_connector.rs"]
mod exchange_connector;
#[path = "headers.rs"]
mod headers;
#[path = "pipeline.rs"]
mod pipeline;
#[path = "target.rs"]
mod target;
#[path = "tunnel.rs"]
mod tunnel;

use body::{ProxyBody, error_response, incoming_body, scheduled_body, text_response};
use config::ForwardPipelineRuntime;
pub use config::{
    ForwardAuthenticationMode, ForwardProxyAuthenticator, ForwardProxyConfig,
    MitmCertificateAuthority, MitmServerIdentity, NoAuthentication,
};
use exchange_connector::ForwardHttpExchangeConnector;
use headers::is_websocket_upgrade;
pub use headers::strip_hop_by_hop_headers;
use pipeline::{
    DropResponseMode, collect_pipeline_body, completion_body, drain_upstream_body,
    drop_response_mode, intentional_drop_error, intentional_response_drop,
    response_from_pipeline_disposition, send_request_then_drop_after_write,
};
pub use target::absolute_uri_to_origin_form;
pub(crate) use target::authority_is_allowed;
use target::{HttpTarget, absolute_http_target};
use tunnel::{connect_target, timeout_or_cancel};

#[derive(Debug, Clone)]
pub struct ForwardProxyService {
    config: ForwardProxyConfig,
    authenticator: Arc<dyn ForwardProxyAuthenticator>,
    pipeline: Option<Arc<ForwardPipelineRuntime>>,
    downstream_tls: Option<DownstreamTlsAcceptor>,
}

struct ForwardConnectionExchange {
    exchange: Arc<crate::http::HttpExchangeConnection>,
}

type SharedForwardExchange = Arc<AsyncMutex<Option<ForwardConnectionExchange>>>;

impl ForwardProxyService {
    pub fn new(
        config: ForwardProxyConfig,
        authenticator: Arc<dyn ForwardProxyAuthenticator>,
    ) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            authenticator,
            pipeline: None,
            downstream_tls: None,
        })
    }

    /// 在正向代理 HTTP 解析之前启用与固定监听一致的 TLS/mTLS 下游握手。
    pub fn with_downstream_tls(mut self, settings: &ReverseDownstreamTls) -> Result<Self> {
        self.downstream_tls = Some(DownstreamTlsAcceptor::new(settings)?);
        Ok(self)
    }

    /// 将正向 HTTP/1.1 消息接入与 Reverse Listener 相同的 Exchange/Pipeline。
    ///
    /// 当前架构只接受可解析的 absolute-form HTTP 请求。CONNECT 与 Upgrade 会在任何
    /// 上游连接建立前明确失败，绝不进入 Exchange 外的透明隧道或降级路径。
    #[must_use]
    pub fn with_pipeline(
        mut self,
        channel: ChannelId,
        runtime_epoch: Uuid,
        ports: Arc<dyn PipelinePorts>,
        capabilities: Arc<dyn HttpProtocolCapabilityFactory>,
        limits: MessageLimits,
    ) -> Self {
        self.pipeline = Some(Arc::new(ForwardPipelineRuntime {
            channel,
            runtime_epoch,
            ports,
            capabilities,
            limits,
        }));
        self
    }

    async fn handle(
        &self,
        request: Request<Incoming>,
        peer: SocketAddr,
        context: Option<ConnectionContext>,
        cancellation: CancellationToken,
        task_scope: ConnectionTaskScope,
        exchange: SharedForwardExchange,
    ) -> Result<Response<ProxyBody>> {
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
            return Ok(text_response(
                StatusCode::NOT_IMPLEMENTED,
                "HTTP CONNECT is not supported by the Exchange runtime",
            ));
        }
        match self
            .forward_http(
                request,
                context.as_ref(),
                cancellation,
                &task_scope,
                &exchange,
            )
            .await
        {
            Ok(response) => Ok(response),
            Err(error)
                if intentional_response_drop(&error)
                    || error.message.contains("HTTP connection Endpoint changed")
                    || error.message.contains("HTTP connection Exchange is closed") =>
            {
                Err(error)
            }
            Err(error) => Ok(error_response(&error)),
        }
    }
}

fn spawn_connection_task<F>(
    task_scope: &ConnectionTaskScope,
    task_name: &'static str,
    future: F,
) -> Result<()>
where
    F: std::future::Future<Output = Result<()>> + Send + 'static,
{
    task_scope
        .spawn_owned(async move {
            match future.await {
                Ok(()) => Ok(()),
                Err(error) => {
                    tracing::debug!(
                        code = error.code,
                        message = %error.message,
                        task = task_name,
                        "forward connection child task ended"
                    );
                    Err(ChildTaskError::new(error.code, error.message))
                }
            }
        })
        .map(|_| ())
        .map_err(|_| {
            ProxyError::new(
                ErrorCode::Internal,
                format!("connection task scope closed before {task_name} could start"),
            )
        })
}

#[path = "service/http.rs"]
mod http_flow;
#[path = "service/lifecycle.rs"]
mod lifecycle;

pub(crate) fn config_error(message: impl Into<String>) -> ProxyError {
    ProxyError::new(ErrorCode::ConfigInvalid, message)
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
