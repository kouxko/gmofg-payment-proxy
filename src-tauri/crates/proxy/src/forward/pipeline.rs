//! HTTP 报文进入通用规则管线后的收集、动作执行与响应重建。

use std::time::Duration;

use bytes::Bytes;
use http::Response;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use tokio_util::sync::CancellationToken;

#[path = "pipeline/drop_response.rs"]
mod drop_response;

pub(super) use drop_response::{
    DropResponseMode, completion_body, drain_upstream_body, drop_response_mode,
    intentional_drop_error, intentional_response_drop, send_request_then_drop_after_write,
};

use super::body::{ProxyBody, scheduled_body};
use super::headers::strip_hop_by_hop_headers;
use super::tunnel::timeout_or_cancel;
use crate::fault::ResponseDisposition;
use crate::message::MessageLimits;
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
