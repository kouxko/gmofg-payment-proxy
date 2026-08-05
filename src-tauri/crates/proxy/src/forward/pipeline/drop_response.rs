//! “发送请求但丢弃响应”动作的生命周期边界。

use std::time::Duration;

use http::{Request, Response};
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::client::conn::http1 as client_http1;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::fault::FaultAction;
use crate::{ErrorCode, ProxyError, Result};

use super::super::body::{CompletionBody, ProxyBody};
use super::super::config_error;
use super::super::tunnel::timeout_or_cancel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::forward) enum DropResponseMode {
    AfterRequestWrite,
    AfterUpstreamBody,
}

pub(in crate::forward) fn drop_response_mode(actions: &[FaultAction]) -> Option<DropResponseMode> {
    actions.iter().find_map(|action| match action {
        FaultAction::DropResponse {
            read_upstream: false,
        } => Some(DropResponseMode::AfterRequestWrite),
        FaultAction::DropResponse {
            read_upstream: true,
        } => Some(DropResponseMode::AfterUpstreamBody),
        _ => None,
    })
}

pub(in crate::forward) fn intentional_response_drop(error: &ProxyError) -> bool {
    error.code == ErrorCode::ClientDisconnected.as_str()
        && error.message.contains("response intentionally dropped")
}

pub(in crate::forward) fn intentional_drop_error(scope: &str) -> ProxyError {
    ProxyError::new(
        ErrorCode::ClientDisconnected,
        format!("{scope} response intentionally dropped"),
    )
}

pub(in crate::forward) fn reject_websocket_drop(actions: &[FaultAction]) -> Result<()> {
    if drop_response_mode(actions).is_some() {
        return Err(config_error(
            "DropResponse is not supported for WebSocket Upgrade; use a connection fault after the 101 handshake",
        ));
    }
    Ok(())
}

pub(in crate::forward) fn completion_body(body: ProxyBody) -> (ProxyBody, oneshot::Receiver<()>) {
    let (body, completed) = CompletionBody::new(body);
    (body.boxed_unsync(), completed)
}

pub(in crate::forward) async fn drain_upstream_body(
    mut body: Incoming,
    cancellation: &CancellationToken,
    read_timeout: Duration,
) -> Result<()> {
    timeout_or_cancel(
        read_timeout,
        cancellation,
        async {
            while let Some(frame) = body.frame().await {
                frame.map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
            }
            Ok(())
        },
        ErrorCode::UpstreamReadTimeout,
    )
    .await??;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(in crate::forward) async fn send_request_then_drop_after_write(
    sender: &mut client_http1::SendRequest<ProxyBody>,
    request: Request<ProxyBody>,
    mut body_written: oneshot::Receiver<()>,
    upstream_shutdown: &CancellationToken,
    cancellation: &CancellationToken,
    timeout: Duration,
    scope: &str,
) -> Result<Response<Incoming>> {
    let send = sender.send_request(request);
    tokio::pin!(send);
    tokio::select! {
        written = &mut body_written => {
            written.map_err(|_| ProxyError::new(
                ErrorCode::Io,
                format!("{scope} request body ended before the complete-write boundary"),
            ))?;
        }
        response = timeout_or_cancel(
            timeout,
            cancellation,
            &mut send,
            ErrorCode::UpstreamReadTimeout,
        ) => {
            response?.map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
            timeout_or_cancel(
                timeout,
                cancellation,
                &mut body_written,
                ErrorCode::UpstreamWriteTimeout,
            )
            .await?
            .map_err(|_| ProxyError::new(
                ErrorCode::Io,
                format!("{scope} request body ended before the complete-write boundary"),
            ))?;
        }
    }
    tokio::task::yield_now().await;
    upstream_shutdown.cancel();
    Err(intentional_drop_error(scope))
}
