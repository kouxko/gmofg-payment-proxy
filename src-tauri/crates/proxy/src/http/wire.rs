use super::{
    Arc, Body, Bytes, CancellationToken, Context, Debug, Duration, ErrorCode, Frame, Future,
    PacedBody, PacedBodyError, Pin, Poll, ProxyError, ResponseWriteTracker, SizeHint,
    TrafficSchedule,
};

#[derive(Debug)]
pub(super) struct WireBody {
    inner: PacedBody,
    finish_delay: Option<Pin<Box<tokio::time::Sleep>>>,
    tracker: Option<Arc<ResponseWriteTracker>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IntentionalWireFault {
    IncorrectContentLength,
    TruncatedResponse,
    StreamAborted,
}

impl IntentionalWireFault {
    pub(super) fn error(self) -> ProxyError {
        match self {
            Self::IncorrectContentLength => ProxyError::new(
                ErrorCode::IncorrectContentLength,
                "response sent with intentionally incorrect content-length",
            ),
            Self::TruncatedResponse => ProxyError::new(
                ErrorCode::TruncatedResponse,
                "response intentionally truncated before completion",
            ),
            Self::StreamAborted => ProxyError::new(
                ErrorCode::FaultStreamAborted,
                "response intentionally disconnected during downstream write",
            ),
        }
    }
}

impl WireBody {
    pub(super) fn new(
        data: Bytes,
        claimed_length: usize,
        schedule: TrafficSchedule,
        cancellation: CancellationToken,
    ) -> Self {
        let finish_delay = (data.len() != claimed_length)
            .then(|| Box::pin(tokio::time::sleep(Duration::from_millis(1))));
        Self {
            inner: PacedBody::new(data, claimed_length, schedule, cancellation),
            finish_delay,
            tracker: None,
        }
    }

    pub(super) fn track(&mut self, tracker: Arc<ResponseWriteTracker>) {
        tracker.begin();
        self.tracker = Some(tracker);
    }
}

impl Body for WireBody {
    type Data = Bytes;
    type Error = PacedBodyError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Self::Data>, Self::Error>>> {
        match Pin::new(&mut self.inner).poll_frame(context) {
            Poll::Ready(None) => {}
            outcome => return outcome,
        }
        if let Some(delay) = self.finish_delay.as_mut() {
            if delay.as_mut().poll(context).is_pending() {
                return Poll::Pending;
            }
            self.finish_delay = None;
        }
        if let Some(tracker) = self.tracker.take() {
            tracker.complete();
        }
        Poll::Ready(None)
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
