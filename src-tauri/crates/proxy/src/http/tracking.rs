use super::{
    Arc, AsyncRead, AsyncWrite, AtomicBool, Body, BoxIo, Bytes, CancellationToken, Context, Debug,
    Formatter, Frame, Future, JoinHandle, Notify, Ordering, PacedBody, PacedBodyError, Pin, Poll,
    RawHttp1HeadCapture, ReadBuf, Result, SizeHint, StdMutex, TrafficSchedule, WriteOutcome, io,
    mpsc, oneshot,
};

use crate::{ErrorCode, ProxyError};

type WriteReceiptResult = Result<WriteOutcome<()>>;
type WriteReceiptSender = oneshot::Sender<WriteReceiptResult>;

#[derive(Clone, Debug)]
pub struct UpstreamWriteReceipt {
    sender: Arc<StdMutex<Option<WriteReceiptSender>>>,
}

#[derive(Debug)]
pub struct UpstreamWriteCompletion {
    receiver: oneshot::Receiver<WriteReceiptResult>,
}

pub(crate) fn upstream_write_receipt() -> (UpstreamWriteReceipt, UpstreamWriteCompletion) {
    UpstreamWriteReceipt::channel()
}

impl UpstreamWriteReceipt {
    pub fn channel() -> (Self, UpstreamWriteCompletion) {
        let (sender, receiver) = oneshot::channel();
        (
            Self {
                sender: Arc::new(StdMutex::new(Some(sender))),
            },
            UpstreamWriteCompletion { receiver },
        )
    }

    /// Confirm that the complete request was accepted by the transport's successful flush.
    pub fn confirm_flushed(&self) {
        self.resolve(Ok(WriteOutcome::Committed(())));
    }

    pub(crate) fn response_ready(&self) {
        self.resolve(Ok(WriteOutcome::ReadReady(())));
    }

    pub(crate) fn fail(&self, error: &ProxyError) {
        self.resolve(Err(error.clone()));
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.sender
            .lock()
            .expect("upstream write receipt mutex poisoned")
            .is_some()
    }

    fn resolve(&self, result: WriteReceiptResult) {
        let sender = self
            .sender
            .lock()
            .expect("upstream write receipt mutex poisoned")
            .take();
        if let Some(sender) = sender {
            let _ = sender.send(result);
        }
    }
}

impl UpstreamWriteCompletion {
    pub async fn wait(self, cancellation: &CancellationToken) -> Result<WriteOutcome<()>> {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(ProxyError::new(
                ErrorCode::ProxyStopped,
                "proxy stopped while awaiting upstream request completion",
            )),
            result = self.receiver => result.unwrap_or_else(|_| {
                Err(ProxyError::new(
                    ErrorCode::Internal,
                    "upstream write receipt dropped before completion",
                ))
            }),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct RequestWriteTracker {
    body_complete: AtomicBool,
    request_flushed: AtomicBool,
    flushed: Notify,
}

impl RequestWriteTracker {
    fn mark_body_complete(&self) {
        self.body_complete.store(true, Ordering::Release);
    }

    fn mark_request_flushed(&self) {
        if self.body_complete.load(Ordering::Acquire)
            && !self.request_flushed.swap(true, Ordering::AcqRel)
        {
            self.flushed.notify_waiters();
        }
    }

    pub(super) async fn wait_until_flushed(&self) {
        loop {
            let notified = self.flushed.notified();
            if self.request_flushed.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

#[derive(Debug)]
pub(super) struct TrackedRequestBody {
    inner: PacedBody,
    tracker: Arc<RequestWriteTracker>,
}

impl TrackedRequestBody {
    pub(super) fn new(
        data: Bytes,
        tracker: Arc<RequestWriteTracker>,
        schedule: TrafficSchedule,
        cancellation: CancellationToken,
    ) -> Self {
        let data_len = data.len();
        if data.is_empty() {
            tracker.mark_body_complete();
        }
        Self {
            inner: PacedBody::new(data, data_len, schedule, cancellation),
            tracker,
        }
    }
}

impl Body for TrackedRequestBody {
    type Data = Bytes;
    type Error = PacedBodyError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        let outcome = Pin::new(&mut self.inner).poll_frame(context);
        if matches!(&outcome, Poll::Ready(None))
            || matches!(&outcome, Poll::Ready(Some(Ok(_)))) && self.inner.is_end_stream()
        {
            self.tracker.mark_body_complete();
        }
        outcome
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

pub(super) struct TrackedIo {
    inner: BoxIo,
    tracker: Arc<RequestWriteTracker>,
    response_head: Arc<StdMutex<RawHttp1HeadCapture>>,
    informational_ready: mpsc::UnboundedSender<()>,
    max_head_bytes: usize,
}

impl Debug for TrackedIo {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrackedIo")
            .field("inner", &"<IoStream>")
            .field("tracker", &self.tracker)
            .field("max_head_bytes", &self.max_head_bytes)
            .finish_non_exhaustive()
    }
}

impl TrackedIo {
    pub(super) fn new(
        inner: BoxIo,
        tracker: Arc<RequestWriteTracker>,
        response_head: Arc<StdMutex<RawHttp1HeadCapture>>,
        informational_ready: mpsc::UnboundedSender<()>,
        max_head_bytes: usize,
    ) -> Self {
        Self {
            inner,
            tracker,
            response_head,
            informational_ready,
            max_head_bytes,
        }
    }
}

impl AsyncRead for TrackedIo {
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
                let informational_before;
                let informational_after;
                {
                    let mut response_head = self
                        .response_head
                        .lock()
                        .expect("raw HTTP head capture mutex poisoned");
                    informational_before = response_head.informational.len();
                    response_head.record(&filled[before..], self.max_head_bytes);
                    informational_after = response_head.informational.len();
                }
                if informational_after > informational_before {
                    let _ = self.informational_ready.send(());
                }
            }
        }
        result
    }
}

impl AsyncWrite for TrackedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.inner).poll_flush(context) {
            Poll::Ready(Ok(())) => {
                self.tracker.mark_request_flushed();
                Poll::Ready(Ok(()))
            }
            outcome => outcome,
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

#[derive(Debug)]
pub(super) struct ConnectionTask {
    handle: Option<JoinHandle<()>>,
}

impl ConnectionTask {
    pub(super) fn spawn(
        connection: impl Future<Output = hyper::Result<()>> + Send + 'static,
    ) -> Self {
        let handle = tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(?error, "upstream HTTP/1 connection ended");
            }
        });
        Self {
            handle: Some(handle),
        }
    }

    pub(super) async fn shutdown(mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        handle.abort();
        if let Err(error) = handle.await
            && !error.is_cancelled()
        {
            tracing::error!(?error, "upstream HTTP/1 connection task failed");
        }
    }
}

impl Drop for ConnectionTask {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}
