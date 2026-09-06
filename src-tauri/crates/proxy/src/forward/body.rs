//! Hyper Body 适配、流量调度 Body 与标准代理错误响应。

use std::error::Error;

use bytes::Bytes;
use http::{Response, StatusCode};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use tokio_util::sync::CancellationToken;

use crate::ProxyError;
use crate::traffic::{PacedBody, TrafficSchedule};

pub(super) type BoxError = Box<dyn Error + Send + Sync>;
pub(super) type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

pub(super) fn incoming_body(body: Incoming) -> ProxyBody {
    body.map_err(|error| -> BoxError { Box::new(error) })
        .boxed_unsync()
}

pub(super) fn full_body(value: impl Into<Bytes>) -> ProxyBody {
    Full::new(value.into())
        .map_err(|never| -> BoxError { match never {} })
        .boxed_unsync()
}

pub(super) fn scheduled_body(
    value: Bytes,
    claimed_length: usize,
    schedule: TrafficSchedule,
    cancellation: &CancellationToken,
) -> ProxyBody {
    if schedule.is_passthrough() {
        return full_body(value);
    }
    PacedBody::new(value, claimed_length, schedule, cancellation.clone())
        .map_err(|error| -> BoxError { Box::new(error) })
        .boxed_unsync()
}

pub(super) fn text_response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(full_body(message.to_owned()))
        .expect("static response is valid")
}

pub(super) fn error_response(error: &ProxyError) -> Response<ProxyBody> {
    let status = match error.code {
        "CONFIG_INVALID" => StatusCode::BAD_REQUEST,
        "UPSTREAM_CONNECT_TIMEOUT" | "UPSTREAM_READ_TIMEOUT" | "UPSTREAM_WRITE_TIMEOUT" => {
            StatusCode::GATEWAY_TIMEOUT
        }
        _ => StatusCode::BAD_GATEWAY,
    };
    text_response(status, &error.message)
}

pub(super) fn tracked_response_body(
    body: ProxyBody,
    tracker: std::sync::Arc<crate::http::ResponseWriteTracker>,
    completed: Option<tokio::sync::oneshot::Sender<crate::Result<()>>>,
    suppressed: bool,
    terminal_write: bool,
) -> crate::Result<ProxyBody> {
    crate::http::ResponseBody::new(body, tracker, completed, suppressed, terminal_write)
        .map(BodyExt::boxed_unsync)
}
