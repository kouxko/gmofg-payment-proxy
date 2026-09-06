//! HTTP 报文进入通用规则管线后的收集、动作执行与响应重建。

use std::time::Duration;

use bytes::Bytes;
use http::Response;
use hyper::body::Incoming;
use tokio_util::sync::CancellationToken;

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
    timeout_or_cancel(
        read_timeout,
        cancellation,
        crate::http::collect_limited(body, limits.max_body_bytes),
        ErrorCode::UpstreamReadTimeout,
    )
    .await?
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
