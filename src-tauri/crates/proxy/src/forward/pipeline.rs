//! HTTP 报文进入通用规则管线后的收集、动作执行与响应重建。

use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, Method, Response, StatusCode, Uri};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use tokio_util::sync::CancellationToken;

#[path = "pipeline/drop_response.rs"]
mod drop_response;

pub(super) use drop_response::{
    DropResponseMode, completion_body, drain_upstream_body, drop_response_mode,
    intentional_drop_error, intentional_response_drop, reject_websocket_drop,
    send_request_then_drop_after_write,
};

use super::ForwardPipelineRuntime;
use super::body::{ProxyBody, scheduled_body};
use super::headers::{ensure_websocket_upgrade_headers, strip_hop_by_hop_headers};
use super::tunnel::timeout_or_cancel;
use crate::fault::{self, FaultAction, ResponseDisposition};
use crate::message::{Message, MessageLimits};
use crate::traffic::TrafficSchedule;
use crate::transport::ConnectionContext;
use crate::{ErrorCode, ProxyError, Result};

pub(super) async fn collect_pipeline_body(
    body: Incoming,
    limits: MessageLimits,
    cancellation: &CancellationToken,
    read_timeout: Duration,
) -> Result<Bytes> {
    let collected = timeout_or_cancel(
        read_timeout,
        cancellation,
        body.collect(),
        ErrorCode::UpstreamReadTimeout,
    )
    .await?
    .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?
    .to_bytes();
    if collected.len() > limits.max_body_bytes {
        return Err(ProxyError::new(
            ErrorCode::BodyTooLarge,
            format!(
                "forward proxy body is {} bytes; limit is {}",
                collected.len(),
                limits.max_body_bytes
            ),
        ));
    }
    Ok(collected)
}

pub(super) async fn prepare_pipeline_request(
    pipeline: &ForwardPipelineRuntime,
    context: &ConnectionContext,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: Bytes,
    cancellation: &CancellationToken,
) -> Result<(Message, Vec<FaultAction>)> {
    let mut message = Message::request(method, uri, headers, body);
    message.validate(pipeline.limits)?;
    let actions = pipeline.ports.request(context, &mut message).await?;
    message.validate(pipeline.limits)?;
    for action in &actions {
        match action {
            FaultAction::Delay(duration) => {
                fault::cancellable_delay(*duration, cancellation).await?;
            }
            FaultAction::DisconnectBeforeUpstream => {
                return Err(ProxyError::new(
                    ErrorCode::ClientDisconnected,
                    "forward request intentionally disconnected before upstream",
                ));
            }
            FaultAction::RejectTls => {
                return Err(ProxyError::new(
                    ErrorCode::TlsHandshakeFailed,
                    "forward request intentionally rejected",
                ));
            }
            FaultAction::UpstreamConnectTimeout(duration)
            | FaultAction::UpstreamWriteTimeout(duration)
            | FaultAction::UpstreamReadTimeout(duration) => {
                fault::cancellable_delay(*duration, cancellation).await?;
                return Err(ProxyError::new(
                    match action {
                        FaultAction::UpstreamConnectTimeout(_) => ErrorCode::UpstreamConnectTimeout,
                        FaultAction::UpstreamWriteTimeout(_) => ErrorCode::UpstreamWriteTimeout,
                        _ => ErrorCode::UpstreamReadTimeout,
                    },
                    "forward request injected timeout completed",
                ));
            }
            _ => {}
        }
    }
    Ok((message, actions))
}

pub(super) fn request_terminal_response(
    actions: &[FaultAction],
    cancellation: &CancellationToken,
) -> Result<Option<Response<ProxyBody>>> {
    for action in actions {
        if let FaultAction::MockResponse {
            status,
            headers,
            body,
        } = action
        {
            let message = fault::mock_response(*status, headers, body.clone());
            return response_from_pipeline_disposition(
                ResponseDisposition::Send {
                    message,
                    schedule: TrafficSchedule::default(),
                },
                cancellation,
            );
        }
    }
    if cancellation.is_cancelled() {
        return Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "forward pipeline cancelled",
        ));
    }
    Ok(None)
}

pub(super) async fn finish_pipeline_response(
    pipeline: &ForwardPipelineRuntime,
    context: &ConnectionContext,
    response: Response<Incoming>,
    cancellation: &CancellationToken,
    read_timeout: Duration,
) -> Result<Response<ProxyBody>> {
    let (mut parts, body) = response.into_parts();
    strip_hop_by_hop_headers(&mut parts.headers);
    let body = collect_pipeline_body(body, pipeline.limits, cancellation, read_timeout).await?;
    let mut message = Message::response(parts.status, &parts.headers, body);
    message.validate(pipeline.limits)?;
    let actions = pipeline.ports.response(context, &mut message).await?;
    message.validate(pipeline.limits)?;
    let disposition = fault::apply_response_actions(message, &actions, cancellation).await?;
    response_from_pipeline_disposition(disposition, cancellation)?.ok_or_else(|| {
        ProxyError::new(
            ErrorCode::ClientDisconnected,
            "forward response intentionally dropped",
        )
    })
}

pub(super) fn response_from_pipeline_disposition(
    disposition: ResponseDisposition,
    cancellation: &CancellationToken,
) -> Result<Option<Response<ProxyBody>>> {
    let (message, body, schedule) = match disposition {
        ResponseDisposition::Send { message, schedule } => {
            let body = message.body.clone();
            (message, body, schedule)
        }
        ResponseDisposition::Drop => return Ok(None),
        ResponseDisposition::Truncate {
            message,
            bytes,
            schedule,
        } => {
            let body = message.body.slice(..bytes);
            (message, body, schedule)
        }
    };
    let status = message.http_status().ok_or_else(|| {
        ProxyError::new(
            ErrorCode::Internal,
            "pipeline response has an invalid HTTP status line",
        )
    })?;
    let mut headers = message.header_map()?;
    strip_hop_by_hop_headers(&mut headers);
    let claimed_length = message.declared_content_length().unwrap_or(body.len());
    let mut response = Response::builder()
        .status(status)
        .body(scheduled_body(body, claimed_length, schedule, cancellation))
        .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
    *response.headers_mut() = headers;
    Ok(Some(response))
}

pub(super) async fn record_websocket_response(
    pipeline: &ForwardPipelineRuntime,
    context: &ConnectionContext,
    parts: http::response::Parts,
    cancellation: &CancellationToken,
) -> Result<http::response::Parts> {
    let mut message = Message::response(parts.status, &parts.headers, Bytes::new());
    message.validate(pipeline.limits)?;
    let actions = pipeline.ports.response(context, &mut message).await?;
    message.validate(pipeline.limits)?;
    let disposition = fault::apply_response_actions(message, &actions, cancellation).await?;
    let response =
        response_from_pipeline_disposition(disposition, cancellation)?.ok_or_else(|| {
            ProxyError::new(
                ErrorCode::ClientDisconnected,
                "WebSocket handshake response intentionally dropped",
            )
        })?;
    let (mut parts, _body) = response.into_parts();
    if parts.status == StatusCode::SWITCHING_PROTOCOLS {
        ensure_websocket_upgrade_headers(&mut parts.headers);
    }
    Ok(parts)
}
