//! 标准 HTTP/1.1 正向代理与 CONNECT 隧道的运行时实现。
//!
//! 该模块不依赖 Tauri、产品配置或证书存储。Host 负责把经过领域校验的监听配置和认证
//! 适配器注入进来；运行时负责协议语义、目标连接、背压、half-close、超时和取消。
//! HTTPS MITM 会在独立 TLS 适配层显式启用；本模块的 CONNECT 默认始终是透明隧道。

#[cfg(test)]
use std::convert::Infallible;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use http::header::{HOST, HeaderValue, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION};
use http::{Method, Request, Response, StatusCode, Uri};
use http_body_util::BodyExt;
#[cfg(test)]
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::client::conn::http1 as client_http1;
use hyper::server::conn::http1 as server_http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
#[cfg(test)]
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::http::{PipelinePorts, traffic_schedule};
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
#[path = "headers.rs"]
mod headers;
#[path = "pipeline.rs"]
mod pipeline;
#[path = "target.rs"]
mod target;
#[path = "tunnel.rs"]
mod tunnel;

use body::{
    ProxyBody, empty_response, error_response, full_body, incoming_body, scheduled_body,
    text_response,
};
pub use config::{
    ForwardAuthenticationMode, ForwardMitmConfig, ForwardProxyAuthenticator, ForwardProxyConfig,
    MitmCertificateAuthority, MitmServerIdentity, MitmUpstreamConnector, NativeRootMitmConnector,
    NoAuthentication,
};
use config::{ForwardMitmRuntime, ForwardPipelineRuntime};
pub use headers::strip_hop_by_hop_headers;
use headers::{
    ensure_websocket_upgrade_headers, is_websocket_upgrade,
    strip_hop_by_hop_headers_preserving_upgrade,
};
use pipeline::{
    DropResponseMode, collect_pipeline_body, completion_body, drain_upstream_body,
    drop_response_mode, finish_pipeline_response, intentional_drop_error,
    intentional_response_drop, prepare_pipeline_request, record_websocket_response,
    reject_websocket_drop, request_terminal_response, send_request_then_drop_after_write,
};
pub use target::absolute_uri_to_origin_form;
pub(crate) use target::authority_is_allowed;
use target::{HttpTarget, absolute_http_target, authority_host, connect_authority};
use tunnel::{
    PrefixIo, client_hello_requires_tunnel, connect_target, read_client_hello_prefix, run_tunnel,
    timeout_or_cancel,
};

#[derive(Debug, Clone)]
pub struct ForwardProxyService {
    config: ForwardProxyConfig,
    authenticator: Arc<dyn ForwardProxyAuthenticator>,
    mitm: Option<Arc<ForwardMitmRuntime>>,
    pipeline: Option<Arc<ForwardPipelineRuntime>>,
    downstream_tls: Option<DownstreamTlsAcceptor>,
}

impl ForwardProxyService {
    pub fn new(
        config: ForwardProxyConfig,
        authenticator: Arc<dyn ForwardProxyAuthenticator>,
    ) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            authenticator,
            mitm: None,
            pipeline: None,
            downstream_tls: None,
        })
    }

    /// 在正向代理 HTTP 解析之前启用与固定监听一致的 TLS/mTLS 下游握手。
    pub fn with_downstream_tls(mut self, settings: &ReverseDownstreamTls) -> Result<Self> {
        self.downstream_tls = Some(DownstreamTlsAcceptor::new(settings)?);
        Ok(self)
    }

    /// 将可解析的正向 HTTP/1.1 消息接入与 Reverse Listener 相同的应用管线。
    ///
    /// CONNECT tunnel 本身不作为 HTTP 业务报文进入管线；只有 absolute-form HTTP 和
    /// allowlist 命中的 MITM 内层 HTTP/1.1 请求进入。未命中 allowlist 的 TLS/h2/h3
    /// 字节流继续透明转发。
    #[must_use]
    pub fn with_pipeline(
        mut self,
        channel: ChannelId,
        runtime_epoch: Uuid,
        ports: Arc<dyn PipelinePorts>,
        limits: MessageLimits,
    ) -> Self {
        self.pipeline = Some(Arc::new(ForwardPipelineRuntime {
            channel,
            runtime_epoch,
            ports,
            limits,
        }));
        self
    }

    /// 为显式 authority 允许列表启用 HTTPS MITM。
    ///
    /// 未命中允许列表的 CONNECT 仍严格走原始透明 tunnel。该方法不改变默认构造行为，
    /// 因此未提供安装级 CA 时不可能意外拦截 HTTPS。
    pub fn with_mitm(
        mut self,
        config: ForwardMitmConfig,
        certificate_authority: Arc<dyn MitmCertificateAuthority>,
        upstream_connector: Arc<dyn MitmUpstreamConnector>,
    ) -> Result<Self> {
        config.validate()?;
        self.mitm = Some(Arc::new(ForwardMitmRuntime::new(
            config,
            certificate_authority,
            upstream_connector,
        )));
        Ok(self)
    }

    async fn handle(
        &self,
        mut request: Request<Incoming>,
        peer: SocketAddr,
        context: Option<ConnectionContext>,
        cancellation: CancellationToken,
        task_scope: ConnectionTaskScope,
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
            return Ok(self
                .handle_connect(&mut request, context, cancellation, &task_scope)
                .await
                .unwrap_or_else(|error| error_response(&error)));
        }
        match self
            .forward_http(request, context.as_ref(), cancellation, &task_scope)
            .await
        {
            Ok(response) => Ok(response),
            Err(error) if intentional_response_drop(&error) => Err(error),
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

#[path = "service/connect.rs"]
mod connect;
#[path = "service/http.rs"]
mod http_flow;
#[path = "service/lifecycle.rs"]
mod lifecycle;
use lifecycle::drain_connection_scope;
#[path = "mitm.rs"]
mod mitm;
#[path = "service/websocket.rs"]
mod websocket;

pub(crate) fn config_error(message: impl Into<String>) -> ProxyError {
    ProxyError::new(ErrorCode::ConfigInvalid, message)
}

fn tls_config_error(error: impl std::fmt::Display) -> ProxyError {
    ProxyError::new(
        ErrorCode::CertificateInvalid,
        format!("MITM TLS configuration failed: {error}"),
    )
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
