use super::{
    Arc, AsyncRead, AsyncWrite, BoxIo, Bytes, CancellationToken, Context, Debug, Duration,
    ErrorCode, Formatter, Pin, Poll, ProxyError, ReadBuf, ReadHalf, Result, StdMutex, WriteHalf,
    informational_status, io, poll_fn, timeout_stage,
};
pub(super) type SharedWriteHalf = Arc<StdMutex<WriteHalf<BoxIo>>>;

pub(super) struct SplitIo {
    pub(super) reader: ReadHalf<BoxIo>,
    pub(super) writer: SharedWriteHalf,
}

impl AsyncRead for SplitIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(context, buffer)
    }
}

impl AsyncWrite for SplitIo {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut writer = self
            .writer
            .lock()
            .expect("downstream HTTP writer mutex poisoned");
        Pin::new(&mut *writer).poll_write(context, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut writer = self
            .writer
            .lock()
            .expect("downstream HTTP writer mutex poisoned");
        Pin::new(&mut *writer).poll_flush(context)
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut writer = self
            .writer
            .lock()
            .expect("downstream HTTP writer mutex poisoned");
        Pin::new(&mut *writer).poll_shutdown(context)
    }
}

#[derive(Clone)]
pub struct InformationalResponseSink {
    writer: SharedWriteHalf,
    write_timeout: Duration,
}

impl Debug for InformationalResponseSink {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InformationalResponseSink")
            .field("write_timeout", &self.write_timeout)
            .finish_non_exhaustive()
    }
}

impl InformationalResponseSink {
    pub(super) fn new(writer: SharedWriteHalf, write_timeout: Duration) -> Self {
        Self {
            writer,
            write_timeout,
        }
    }

    pub async fn publish(&self, head: Bytes, cancellation: &CancellationToken) -> Result<()> {
        if informational_status(&head).is_none() {
            return Err(ProxyError::new(
                ErrorCode::Internal,
                "only informational HTTP response heads may be published early",
            ));
        }
        let writer = Arc::clone(&self.writer);
        timeout_stage(
            self.write_timeout,
            cancellation,
            async move {
                let mut offset = 0usize;
                while offset < head.len() {
                    let written = poll_fn(|context| {
                        let mut writer = writer
                            .lock()
                            .expect("downstream HTTP writer mutex poisoned");
                        Pin::new(&mut *writer).poll_write(context, &head[offset..])
                    })
                    .await?;
                    if written == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "failed to write informational HTTP response head",
                        ));
                    }
                    offset += written;
                }
                poll_fn(|context| {
                    let mut writer = writer
                        .lock()
                        .expect("downstream HTTP writer mutex poisoned");
                    Pin::new(&mut *writer).poll_flush(context)
                })
                .await
            },
            ErrorCode::Io,
        )
        .await?
        .map_err(|error| ProxyError::io("write informational HTTP response downstream", &error))
    }
}
