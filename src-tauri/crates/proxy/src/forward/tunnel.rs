//! CONNECT 字节隧道、TLS `ClientHello` 预读与可取消超时工具。

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::{ErrorCode, ProxyError, Result};

#[path = "tunnel/client_hello.rs"]
mod client_hello;

pub(super) use client_hello::{client_hello_requires_tunnel, read_client_hello_prefix};

const COPY_BUFFER_BYTES: usize = 16 * 1024;

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

#[derive(Debug)]
pub(super) struct PrefixIo<T> {
    prefix: Bytes,
    offset: usize,
    inner: T,
}

impl<T> PrefixIo<T> {
    pub(super) fn new(prefix: Bytes, inner: T) -> Self {
        Self {
            prefix,
            offset: 0,
            inner,
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for PrefixIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.offset < self.prefix.len() && buffer.remaining() > 0 {
            let available = &self.prefix[self.offset..];
            let count = available.len().min(buffer.remaining());
            buffer.put_slice(&available[..count]);
            self.offset += count;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for PrefixIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

pub(super) async fn run_tunnel<A, U>(
    downstream: A,
    upstream: U,
    idle_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (downstream_read, downstream_write) = tokio::io::split(downstream);
    let (upstream_read, upstream_write) = tokio::io::split(upstream);
    let upstream_copy = copy_direction(
        downstream_read,
        upstream_write,
        idle_timeout,
        cancellation.clone(),
    );
    let downstream_copy =
        copy_direction(upstream_read, downstream_write, idle_timeout, cancellation);
    tokio::try_join!(upstream_copy, downstream_copy)?;
    Ok(())
}

pub(super) async fn copy_direction<R, W>(
    mut reader: R,
    mut writer: W,
    idle_timeout: Duration,
    cancellation: CancellationToken,
) -> Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut copied = 0u64;
    let mut buffer = vec![0; COPY_BUFFER_BYTES];
    loop {
        let read = timeout_or_cancel(
            idle_timeout,
            &cancellation,
            reader.read(&mut buffer),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("read CONNECT tunnel", &error))?;
        if read == 0 {
            timeout_or_cancel(
                idle_timeout,
                &cancellation,
                writer.shutdown(),
                ErrorCode::UpstreamWriteTimeout,
            )
            .await?
            .map_err(|error| ProxyError::io("half-close CONNECT tunnel", &error))?;
            return Ok(copied);
        }
        timeout_or_cancel(
            idle_timeout,
            &cancellation,
            writer.write_all(&buffer[..read]),
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("write CONNECT tunnel", &error))?;
        copied = copied.saturating_add(read as u64);
    }
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
