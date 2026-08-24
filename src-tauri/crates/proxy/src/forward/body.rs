//! Hyper Body 适配、流量调度 Body 与标准代理错误响应。

use std::error::Error;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes};
use http::{Response, StatusCode};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Frame, Incoming, SizeHint};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::ProxyError;
use crate::traffic::{PacedBody, TrafficSchedule};

pub(super) type BoxError = Box<dyn Error + Send + Sync>;
pub(super) type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

/// 在 Hyper 已经完整消费请求 Body 时发出一次通知。
pub(super) struct CompletionBody<B> {
    inner: B,
    completed: Option<oneshot::Sender<()>>,
    remaining: Option<u64>,
}

impl<B: Body> CompletionBody<B> {
    pub(super) fn new(inner: B) -> (Self, oneshot::Receiver<()>) {
        let (completed, receiver) = oneshot::channel();
        let remaining = inner.size_hint().exact();
        (
            Self {
                inner,
                completed: Some(completed),
                remaining,
            },
            receiver,
        )
    }
}

impl<B> Body for CompletionBody<B>
where
    B: Body + Unpin,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        let result = Pin::new(&mut self.inner).poll_frame(context);
        if let Poll::Ready(Some(Ok(frame))) = &result
            && let (Some(remaining), Some(data)) = (self.remaining.as_mut(), frame.data_ref())
        {
            *remaining = remaining.saturating_sub(data.remaining() as u64);
        }
        let fully_consumed = matches!(result, Poll::Ready(None))
            || self.remaining.is_some_and(|remaining| remaining == 0);
        if fully_consumed && let Some(completed) = self.completed.take() {
            let _ = completed.send(());
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.completed.is_none() && self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

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
