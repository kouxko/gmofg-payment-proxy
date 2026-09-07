//! One response completion boundary shared by fixed and forward HTTP transports.

use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use bytes::Buf;
use hyper::body::{Body, Frame, SizeHint};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::{Notify, oneshot},
};
use tokio_util::sync::CancellationToken;

use crate::{ErrorCode, ProxyError, Result, transport::BoxIo};

#[derive(Debug, Default)]
pub(crate) struct ResponseWriteTracker {
    state: Mutex<Option<PendingResponseWrite>>,
    changed: Notify,
}

#[derive(Debug)]
struct PendingResponseWrite {
    started: bool,
    body_complete: bool,
    defer_until_close: bool,
    completed: Option<oneshot::Sender<Result<()>>>,
}

impl ResponseWriteTracker {
    fn begin(
        &self,
        body_complete: bool,
        defer_until_close: bool,
        completed: Option<oneshot::Sender<Result<()>>>,
    ) -> Result<()> {
        let mut state = self.state.lock().expect("response write state poisoned");
        if state.is_some() {
            return Err(ProxyError::new(
                ErrorCode::Internal,
                "previous HTTP response write is still pending",
            ));
        }
        *state = Some(PendingResponseWrite {
            started: false,
            body_complete,
            defer_until_close,
            completed,
        });
        self.changed.notify_waiters();
        Ok(())
    }

    fn body_complete(&self) {
        if let Some(state) = self
            .state
            .lock()
            .expect("response write state poisoned")
            .as_mut()
        {
            state.body_complete = true;
        }
    }

    fn started(&self) {
        if let Some(state) = self
            .state
            .lock()
            .expect("response write state poisoned")
            .as_mut()
        {
            state.started = true;
        }
    }

    fn flushed(&self) {
        let completed = {
            let mut state = self.state.lock().expect("response write state poisoned");
            if !state.as_ref().is_some_and(|pending| {
                pending.started && pending.body_complete && !pending.defer_until_close
            }) {
                return;
            }
            state.take().and_then(|pending| pending.completed)
        };
        if let Some(completed) = completed {
            let _ = completed.send(Ok(()));
        }
        self.changed.notify_waiters();
    }

    pub(crate) fn fail(&self, error: &ProxyError) {
        let completed = {
            let mut state = self.state.lock().expect("response write state poisoned");
            state.take().and_then(|pending| pending.completed)
        };
        if let Some(completed) = completed {
            let _ = completed.send(Err(error.clone()));
        }
        self.changed.notify_waiters();
    }

    pub(crate) async fn wait_for_pending(&self) {
        self.wait_for(true).await;
    }
    pub(crate) async fn wait_for_clear(&self) {
        self.wait_for(false).await;
    }

    async fn wait_for(&self, pending: bool) {
        loop {
            let changed = self.changed.notified();
            if self
                .state
                .lock()
                .expect("response write state poisoned")
                .is_some()
                == pending
            {
                return;
            }
            changed.await;
        }
    }
}

#[derive(Debug)]
pub(crate) struct ResponseBody<B> {
    inner: B,
    remaining: Option<u64>,
    tracker: Arc<ResponseWriteTracker>,
}

impl<B: Body> ResponseBody<B> {
    pub(crate) fn new(
        inner: B,
        tracker: Arc<ResponseWriteTracker>,
        completed: Option<oneshot::Sender<Result<()>>>,
        body_suppressed: bool,
        defer_until_close: bool,
    ) -> Result<Self> {
        let remaining = inner.size_hint().exact();
        tracker.begin(
            body_suppressed || inner.is_end_stream() || remaining == Some(0),
            defer_until_close,
            completed,
        )?;
        Ok(Self {
            inner,
            remaining,
            tracker,
        })
    }
}

impl<B: Body + Unpin + 'static> Body for ResponseBody<B> {
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
        if matches!(result, Poll::Ready(None)) || self.remaining == Some(0) {
            self.tracker.body_complete();
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

pub(crate) struct ResponseIo {
    inner: BoxIo,
    tracker: Arc<ResponseWriteTracker>,
}

impl ResponseIo {
    pub(crate) fn new(inner: BoxIo, tracker: Arc<ResponseWriteTracker>) -> Self {
        Self { inner, tracker }
    }
}

impl std::fmt::Debug for ResponseIo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseIo").finish_non_exhaustive()
    }
}

impl AsyncRead for ResponseIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for ResponseIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(context, buffer);
        if matches!(result, Poll::Ready(Ok(written)) if written > 0) {
            self.tracker.started();
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let result = Pin::new(&mut self.inner).poll_flush(context);
        if matches!(result, Poll::Ready(Ok(()))) {
            self.tracker.flushed();
        }
        result
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

/// Poll the HTTP engine concurrently with cancellation and each response's physical write deadline.
pub(crate) async fn drive_http1<F: Future + Send + 'static>(
    connection: F,
    tracker: &ResponseWriteTracker,
    cancellation: &CancellationToken,
    write_timeout: Duration,
) -> Result<F::Output> {
    tokio::pin!(connection);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => return Err(ProxyError::new(ErrorCode::ProxyStopped, "proxy stopped while HTTP connection was active")),
            result = &mut connection => return Ok(result),
            () = tracker.wait_for_pending() => {
                tokio::select! {
                    result = &mut connection => return Ok(result),
                    () = tracker.wait_for_clear() => {},
                    () = cancellation.cancelled() => return Err(ProxyError::new(ErrorCode::ProxyStopped, "proxy stopped during downstream response write")),
                    () = tokio::time::sleep(write_timeout) => return Err(ProxyError::new(ErrorCode::Io, format!("downstream response write timed out after {} ms", write_timeout.as_millis()))),
                }
            }
        }
    }
}
