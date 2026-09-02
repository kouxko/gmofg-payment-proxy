//! 正向 HTTP 目标到通用 `UpstreamConnector` 的适配。

use std::{fmt, time::Duration};

use async_trait::async_trait;
use http::header::{CONTENT_LENGTH, HOST, HeaderValue};
use http::{Request, Response};
use hyper::client::conn::http1 as client_http1;
use hyper_util::rt::TokioIo;
use tokio_util::sync::CancellationToken;

use super::{
    ConnectionTaskScope, DropResponseMode, ErrorCode, MessageLimits, ProxyError, TrafficDirection,
    collect_pipeline_body, completion_body, connect_target, drain_upstream_body,
    drop_response_mode, intentional_drop_error, scheduled_body, send_request_then_drop_after_write,
    spawn_connection_task, strip_hop_by_hop_headers, timeout_or_cancel, traffic_schedule,
};
use crate::Result;
use crate::fault::FaultAction;
use crate::http::{
    ForwardRequest, InformationalResponseSink, PipelinePorts, UpstreamConnector, UpstreamExchange,
};
use crate::message::Message;
use crate::transport::ConnectionContext;

#[derive(Clone)]
pub(super) struct ForwardHttpExchangeConnector {
    pub(super) connect_authority: String,
    pub(super) host_header: String,
    pub(super) connect_timeout: Duration,
    pub(super) write_timeout: Duration,
    pub(super) read_timeout: Duration,
    pub(super) limits: MessageLimits,
    pub(super) task_scope: ConnectionTaskScope,
}

impl fmt::Debug for ForwardHttpExchangeConnector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ForwardHttpExchangeConnector")
            .field("connect_authority", &self.connect_authority)
            .field("host_header", &self.host_header)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl UpstreamConnector for ForwardHttpExchangeConnector {
    async fn send(
        &self,
        _context: &ConnectionContext,
        _ports: &dyn PipelinePorts,
        request: ForwardRequest,
        actions: &[FaultAction],
        _informational: Option<&InformationalResponseSink>,
        cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
        apply_injected_timeout(actions, cancellation).await?;
        let mut headers = request.message.header_map()?;
        strip_hop_by_hop_headers(&mut headers);
        if !headers.contains_key(HOST) {
            headers.insert(
                HOST,
                HeaderValue::from_str(&self.host_header).map_err(|error| {
                    ProxyError::new(ErrorCode::ConfigInvalid, error.to_string())
                })?,
            );
        }
        headers.remove(CONTENT_LENGTH);
        headers.insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&request.message.body.len().to_string())
                .map_err(|error| ProxyError::new(ErrorCode::ConfigInvalid, error.to_string()))?,
        );
        let stream =
            connect_target(&self.connect_authority, self.connect_timeout, cancellation).await?;
        let (mut sender, connection) = client_http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        let upstream_shutdown = CancellationToken::new();
        let connection_shutdown = upstream_shutdown.clone();
        spawn_connection_task(
            &self.task_scope,
            "forward Exchange origin connection",
            async move {
                tokio::select! {
                    () = connection_shutdown.cancelled() => Ok(()),
                    result = connection => result.map_err(|error| ProxyError::new(
                        ErrorCode::Io,
                        format!("forward Exchange origin connection ended: {error}"),
                    )),
                }
            },
        )?;
        let schedule = traffic_schedule(actions, TrafficDirection::Upstream)?;
        let timeout = self
            .write_timeout
            .saturating_add(self.read_timeout)
            .saturating_add(schedule.estimated_delay(request.message.body.len()));
        let mode = drop_response_mode(actions);
        let body = scheduled_body(
            request.message.body.clone(),
            request.message.body.len(),
            schedule,
            cancellation,
        );
        let (body, written) = if mode == Some(DropResponseMode::AfterRequestWrite) {
            let (body, completed) = completion_body(body);
            (body, Some(completed))
        } else {
            (body, None)
        };
        let mut outgoing = Request::new(body);
        *outgoing.method_mut() = request.method;
        *outgoing.uri_mut() = request.uri;
        *outgoing.headers_mut() = headers;
        if let Some(written) = written {
            return send_request_then_drop_after_write(
                &mut sender,
                outgoing,
                written,
                &upstream_shutdown,
                cancellation,
                timeout,
                "forward Exchange",
            )
            .await
            .map(|_| unreachable!("drop helper only returns errors"));
        }
        let response = timeout_or_cancel(
            timeout,
            cancellation,
            sender.send_request(outgoing),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        if mode == Some(DropResponseMode::AfterUpstreamBody) {
            drain_upstream_body(response.into_body(), cancellation, self.read_timeout).await?;
            upstream_shutdown.cancel();
            return Err(intentional_drop_error("forward Exchange"));
        }
        response_message(response, self.limits, cancellation, self.read_timeout).await
    }
}

async fn response_message(
    response: Response<hyper::body::Incoming>,
    limits: MessageLimits,
    cancellation: &CancellationToken,
    read_timeout: Duration,
) -> Result<UpstreamExchange> {
    let (mut parts, body) = response.into_parts();
    strip_hop_by_hop_headers(&mut parts.headers);
    let body = collect_pipeline_body(body, limits, cancellation, read_timeout).await?;
    let message = Message::response(parts.status, &parts.headers, body);
    message.validate(limits)?;
    Ok(UpstreamExchange::from(message))
}

async fn apply_injected_timeout(
    actions: &[FaultAction],
    cancellation: &CancellationToken,
) -> Result<()> {
    for action in actions {
        let (duration, code) = match action {
            FaultAction::UpstreamConnectTimeout(duration) => {
                (*duration, ErrorCode::UpstreamConnectTimeout)
            }
            FaultAction::UpstreamWriteTimeout(duration) => {
                (*duration, ErrorCode::UpstreamWriteTimeout)
            }
            FaultAction::UpstreamReadTimeout(duration) => {
                (*duration, ErrorCode::UpstreamReadTimeout)
            }
            _ => continue,
        };
        crate::fault::cancellable_delay(duration, cancellation).await?;
        return Err(ProxyError::new(
            code,
            "forward Exchange injected timeout completed",
        ));
    }
    Ok(())
}
