use super::{
    Arc, AsyncRead, AsyncWrite, BoxIo, Bytes, Context, Debug, Formatter, Pin, Poll, ReadBuf,
    StdMutex, informational_status, io,
};

pub(super) struct ResponseHeadPreservingIo {
    inner: BoxIo,
    generated_head: Vec<u8>,
    canonical_head: Arc<StdMutex<Option<Bytes>>>,
    pending_head: Option<Bytes>,
    canonical_offset: usize,
    generated_head_complete: bool,
    reset_after_flush: bool,
}

impl Debug for ResponseHeadPreservingIo {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResponseHeadPreservingIo")
            .field("canonical_offset", &self.canonical_offset)
            .field("generated_head_complete", &self.generated_head_complete)
            .finish_non_exhaustive()
    }
}

impl ResponseHeadPreservingIo {
    pub(super) fn new(inner: BoxIo, canonical_head: Arc<StdMutex<Option<Bytes>>>) -> Self {
        Self {
            inner,
            generated_head: Vec::new(),
            canonical_head,
            pending_head: None,
            canonical_offset: 0,
            generated_head_complete: false,
            reset_after_flush: false,
        }
    }

    fn poll_flush_canonical(&mut self, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.pending_head.is_none() {
            let published = self
                .canonical_head
                .lock()
                .expect("canonical HTTP response head mutex poisoned")
                .clone();
            self.pending_head.clone_from(&published);
        }
        let Some(canonical) = self.pending_head.clone() else {
            return Poll::Ready(Err(io::Error::other(
                "canonical HTTP response head was not published before write",
            )));
        };
        while self.canonical_offset < canonical.len() {
            let written = match Pin::new(&mut self.inner)
                .poll_write(context, &canonical[self.canonical_offset..])
            {
                Poll::Ready(Ok(written)) => written,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            };
            if written == 0 {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write canonical HTTP response head",
                )));
            }
            self.canonical_offset += written;
        }
        if self.reset_after_flush {
            self.generated_head.clear();
            self.pending_head = None;
            self.canonical_offset = 0;
            self.generated_head_complete = false;
            self.reset_after_flush = false;
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for ResponseHeadPreservingIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for ResponseHeadPreservingIo {
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
            if self.generated_head_complete {
                return Pin::new(&mut self.inner).poll_write(context, buffer);
            }
        }

        let mut consumed = 0usize;
        for byte in buffer {
            self.generated_head.push(*byte);
            consumed += 1;
            if self.generated_head.ends_with(b"\r\n\r\n") {
                self.generated_head_complete = true;
                if informational_status(&self.generated_head).is_some() {
                    self.pending_head = Some(Bytes::copy_from_slice(&self.generated_head));
                    self.reset_after_flush = true;
                }
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
