//! 正向 HTTP 目标连接与可取消超时工具。

use std::{future::Future, time::Duration};

use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::{ErrorCode, ProxyError, Result};

pub(super) async fn connect_target(
    authority: &str,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<TcpStream> {
    let stream = timeout_or_cancel(
        timeout,
        cancellation,
        TcpStream::connect(authority),
        ErrorCode::UpstreamConnectTimeout,
    )
    .await?
    .map_err(|error| ProxyError::io("connect forward proxy target", &error))?;
    stream
        .set_nodelay(true)
        .map_err(|error| ProxyError::io("configure forward proxy target", &error))?;
    Ok(stream)
}

pub(super) async fn timeout_or_cancel<F, T>(
    duration: Duration,
    cancellation: &CancellationToken,
    future: F,
    timeout_code: ErrorCode,
) -> Result<T>
where
    F: Future<Output = T>,
{
    tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "forward proxy operation cancelled",
        )),
        result = tokio::time::timeout(duration, future) => result.map_err(|_| ProxyError::new(
            timeout_code,
            format!("forward proxy operation timed out after {} ms", duration.as_millis()),
        )),
    }
}
