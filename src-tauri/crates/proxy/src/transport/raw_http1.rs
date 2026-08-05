//! HTTP/1 原始 head 捕获与保字节 I/O 包装器。
//!
//! Hyper 拥有语义解析，这些包装器只旁路记录线上字节，用于抓包、重放、1xx 响应和故意
//! 制造协议故障。捕获有严格上限；边界不完整或超限时返回错误，绝不从 Hyper 规范化后的
//! header 反推原始报文。

use std::fmt::{Debug, Formatter};
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::StatusCode;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::BoxIo;
use crate::{ErrorCode, ProxyError, Result};

#[derive(Debug, Default)]
pub(super) struct RawHttp1HeadCapture {
    pending: Vec<u8>,
    complete: Option<Bytes>,
    pub(super) informational: Vec<Bytes>,
    skip_informational: bool,
    limit_exceeded: bool,
    captured_bytes: usize,
}

impl RawHttp1HeadCapture {
    pub(super) fn final_response() -> Self {
        Self {
            skip_informational: true,
            ..Self::default()
        }
    }

    pub(super) fn record(&mut self, bytes: &[u8], max_bytes: usize) {
        for byte in bytes {
            if self.complete.is_some() || self.limit_exceeded {
                return;
            }
            if self.captured_bytes == max_bytes {
                self.limit_exceeded = true;
                return;
            }
            self.pending.push(*byte);
            self.captured_bytes += 1;
            if self.pending.ends_with(b"\r\n\r\n") {
                if self.skip_informational
                    && captured_status(&self.pending).is_some_and(|status| {
                        (100..200).contains(&status)
                            && status != StatusCode::SWITCHING_PROTOCOLS.as_u16()
                    })
                {
                    self.informational
                        .push(Bytes::from(std::mem::take(&mut self.pending)));
                } else {
                    self.complete = Some(Bytes::from(std::mem::take(&mut self.pending)));
                }
            }
        }
    }

    pub(super) fn required_head(&self, direction: &str) -> Result<Bytes> {
        if self.limit_exceeded {
            return Err(ProxyError::new(
                ErrorCode::HeaderLimitExceeded,
                format!("{direction} HTTP/1 head exceeded the capture limit"),
            ));
        }
        self.complete.clone().ok_or_else(|| {
            ProxyError::new(
                ErrorCode::HeaderLimitExceeded,
                format!("{direction} HTTP/1 head was not captured completely"),
            )
        })
    }

    pub(super) fn informational_heads(&self, direction: &str) -> Result<Vec<Bytes>> {
        if self.limit_exceeded {
            return Err(ProxyError::new(
                ErrorCode::HeaderLimitExceeded,
                format!("{direction} informational HTTP heads exceeded the capture limit"),
            ));
        }
        Ok(self.informational.clone())
    }
}

fn captured_status(head: &[u8]) -> Option<u16> {
    let line_end = head.windows(2).position(|window| window == b"\r\n")?;
    let start_line = std::str::from_utf8(&head[..line_end]).ok()?;
    let mut parts = start_line.split_ascii_whitespace();
    parts.next()?.starts_with("HTTP/").then_some(())?;
    parts.next()?.parse().ok()
}

pub(super) struct ReadRecordingIo {
    inner: BoxIo,
    capture: Arc<StdMutex<RawHttp1HeadCapture>>,
    max_head_bytes: usize,
}

impl Debug for ReadRecordingIo {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReadRecordingIo")
            .field("inner", &"<IoStream>")
            .field("max_head_bytes", &self.max_head_bytes)
            .finish_non_exhaustive()
    }
}

impl ReadRecordingIo {
    pub(super) fn new(
        inner: BoxIo,
        capture: Arc<StdMutex<RawHttp1HeadCapture>>,
        max_head_bytes: usize,
    ) -> Self {
        Self {
            inner,
            capture,
            max_head_bytes,
        }
    }

    pub(super) fn into_inner(self) -> BoxIo {
        self.inner
    }
}

impl AsyncRead for ReadRecordingIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(context, buffer);
        if matches!(result, Poll::Ready(Ok(()))) {
            let filled = buffer.filled();
            if filled.len() > before {
                self.capture
                    .lock()
                    .expect("raw HTTP head capture mutex poisoned")
                    .record(&filled[before..], self.max_head_bytes);
            }
        }
        result
    }
}

impl AsyncWrite for ReadRecordingIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(context, buffers)
    }
}

pub(super) struct RequestHeadPreservingIo {
    inner: BoxIo,
    generated_head: Vec<u8>,
    canonical_head: Bytes,
    canonical_offset: usize,
    generated_head_complete: bool,
}

impl Debug for RequestHeadPreservingIo {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RequestHeadPreservingIo")
            .field("canonical_head_bytes", &self.canonical_head.len())
            .field("canonical_offset", &self.canonical_offset)
            .field("generated_head_complete", &self.generated_head_complete)
            .finish_non_exhaustive()
    }
}

impl RequestHeadPreservingIo {
    pub(super) fn new(inner: BoxIo, canonical_head: Bytes) -> Self {
        Self {
            inner,
            generated_head: Vec::new(),
            canonical_head,
            canonical_offset: 0,
            generated_head_complete: false,
        }
    }

    fn poll_flush_canonical(&mut self, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.canonical_offset < self.canonical_head.len() {
            let written = match Pin::new(&mut self.inner)
                .poll_write(context, &self.canonical_head[self.canonical_offset..])
            {
                Poll::Ready(Ok(written)) => written,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            };
            if written == 0 {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write canonical HTTP request head",
                )));
            }
            self.canonical_offset += written;
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for RequestHeadPreservingIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for RequestHeadPreservingIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.generated_head_complete {
            match self.poll_flush_canonical(context) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
            return Pin::new(&mut self.inner).poll_write(context, buffer);
        }

        let mut consumed = 0usize;
        for byte in buffer {
            self.generated_head.push(*byte);
            consumed += 1;
            if self.generated_head.ends_with(b"\r\n\r\n") {
                self.generated_head_complete = true;
                // Only the generated HTTP head is replaced. Returning the
                // prefix count makes Hyper retry any body suffix unchanged.
                break;
            }
        }
        Poll::Ready(Ok(consumed))
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.generated_head_complete {
            match self.poll_flush_canonical(context) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.generated_head_complete {
            match self.poll_flush_canonical(context) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}
