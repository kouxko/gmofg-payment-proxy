//! 正向 HTTP 目标连接与可取消超时工具。

use std::time::Duration;

use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

pub(super) use crate::http::timeout_stage as timeout_or_cancel;
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
