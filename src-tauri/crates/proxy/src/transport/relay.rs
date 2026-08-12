use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::{ErrorCode, ProxyError, Result};

use super::BoxIo;

const RELAY_BUFFER_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RelayDirection {
    ClientToServer,
    ServerToClient,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RelayOperation {
    Read,
    Write,
    Flush,
    HalfClose,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RelayBytes {
    pub(crate) client_to_server: u64,
    pub(crate) server_to_client: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RelayTimeoutCodes {
    pub(crate) read: ErrorCode,
    pub(crate) write: ErrorCode,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RelayTimeouts {
    read: Duration,
    write: Duration,
    codes: RelayTimeoutCodes,
}

impl RelayTimeouts {
    pub(crate) const fn new(read: Duration, write: Duration, codes: RelayTimeoutCodes) -> Self {
        Self { read, write, codes }
    }
}

impl RelayTimeoutCodes {
    pub(crate) const fn upstream() -> Self {
        Self {
            read: ErrorCode::UpstreamReadTimeout,
            write: ErrorCode::UpstreamWriteTimeout,
        }
    }
}

#[derive(Debug)]
pub(crate) struct RelayFailure {
    pub(crate) error: ProxyError,
    pub(crate) direction: RelayDirection,
    pub(crate) operation: RelayOperation,
    pub(crate) bytes: RelayBytes,
}

#[derive(Debug, Default)]
pub(crate) struct RelayProgress {
    client_to_server: AtomicU64,
    server_to_client: AtomicU64,
}

impl RelayProgress {
    pub(crate) fn add(&self, direction: RelayDirection, bytes: usize) {
        let counter = match direction {
            RelayDirection::ClientToServer => &self.client_to_server,
            RelayDirection::ServerToClient => &self.server_to_client,
        };
        counter.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> RelayBytes {
        RelayBytes {
            client_to_server: self.client_to_server.load(Ordering::Relaxed),
            server_to_client: self.server_to_client.load(Ordering::Relaxed),
        }
    }
}

pub(crate) async fn relay_bidirectional(
    downstream: BoxIo,
    upstream: BoxIo,
    timeouts: RelayTimeouts,
    cancellation: CancellationToken,
) -> std::result::Result<RelayBytes, RelayFailure> {
    relay_bidirectional_with_progress(
        downstream,
        upstream,
        timeouts,
        cancellation,
        Arc::new(RelayProgress::default()),
    )
    .await
}

pub(crate) async fn relay_bidirectional_with_progress(
    downstream: BoxIo,
    upstream: BoxIo,
    timeouts: RelayTimeouts,
    cancellation: CancellationToken,
    counters: Arc<RelayProgress>,
) -> std::result::Result<RelayBytes, RelayFailure> {
    let (downstream_read, downstream_write) = tokio::io::split(downstream);
    let (upstream_read, upstream_write) = tokio::io::split(upstream);
    let client_to_server = copy_direction(
        downstream_read,
        upstream_write,
        RelayDirection::ClientToServer,
        timeouts,
        Arc::clone(&counters),
        cancellation.child_token(),
    );
    let server_to_client = copy_direction(
        upstream_read,
        downstream_write,
        RelayDirection::ServerToClient,
        timeouts,
        Arc::clone(&counters),
        cancellation.child_token(),
    );

    if let Err(mut failure) = tokio::try_join!(client_to_server, server_to_client) {
        failure.bytes = counters.snapshot();
        return Err(failure);
    }
    Ok(counters.snapshot())
}

async fn copy_direction<R, W>(
    mut reader: R,
    mut writer: W,
    direction: RelayDirection,
    timeouts: RelayTimeouts,
    counters: Arc<RelayProgress>,
    cancellation: CancellationToken,
) -> std::result::Result<(), RelayFailure>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0_u8; RELAY_BUFFER_BYTES];
    loop {
        let read = relay_io(
            reader.read(&mut buffer),
            timeouts.read,
            &cancellation,
            timeouts.codes.read,
            direction,
            RelayOperation::Read,
            &counters,
        )
        .await?;
        if read == 0 {
            relay_io(
                writer.shutdown(),
                timeouts.write,
                &cancellation,
                timeouts.codes.write,
                direction,
                RelayOperation::HalfClose,
                &counters,
            )
            .await?;
            return Ok(());
        }

        let mut offset = 0;
        while offset < read {
            let written = relay_io(
                writer.write(&buffer[offset..read]),
                timeouts.write,
                &cancellation,
                timeouts.codes.write,
                direction,
                RelayOperation::Write,
                &counters,
            )
            .await?;
            if written == 0 {
                let error =
                    std::io::Error::new(std::io::ErrorKind::WriteZero, "relay write returned zero");
                return Err(io_failure(
                    &error,
                    direction,
                    RelayOperation::Write,
                    &counters,
                ));
            }
            offset += written;
            counters.add(direction, written);
        }
        relay_io(
            writer.flush(),
            timeouts.write,
            &cancellation,
            timeouts.codes.write,
            direction,
            RelayOperation::Flush,
            &counters,
        )
        .await?;
    }
}

async fn relay_io<F, T>(
    future: F,
    duration: Duration,
    cancellation: &CancellationToken,
    timeout_code: ErrorCode,
    direction: RelayDirection,
    operation: RelayOperation,
    counters: &RelayProgress,
) -> std::result::Result<T, RelayFailure>
where
    F: Future<Output = std::io::Result<T>>,
{
    timeout_cancel_first(
        duration,
        cancellation,
        future,
        timeout_code,
        "transport relay cancelled",
        "transport relay I/O",
    )
    .await
    .map_err(|error| RelayFailure {
        error,
        direction,
        operation,
        bytes: counters.snapshot(),
    })?
    .map_err(|error| io_failure(&error, direction, operation, counters))
}

fn io_failure(
    error: &std::io::Error,
    direction: RelayDirection,
    operation: RelayOperation,
    counters: &RelayProgress,
) -> RelayFailure {
    RelayFailure {
        error: ProxyError::io(io_context(direction, operation), error),
        direction,
        operation,
        bytes: counters.snapshot(),
    }
}

fn io_context(direction: RelayDirection, operation: RelayOperation) -> &'static str {
    match (direction, operation) {
        (RelayDirection::ClientToServer, RelayOperation::Read) => {
            "read client-to-server relay stream"
        }
        (RelayDirection::ClientToServer, RelayOperation::Write) => {
            "write client-to-server relay stream"
        }
        (RelayDirection::ClientToServer, RelayOperation::Flush) => {
            "flush client-to-server relay stream"
        }
        (RelayDirection::ClientToServer, RelayOperation::HalfClose) => {
            "half-close client-to-server relay stream"
        }
        (RelayDirection::ServerToClient, RelayOperation::Read) => {
            "read server-to-client relay stream"
        }
        (RelayDirection::ServerToClient, RelayOperation::Write) => {
            "write server-to-client relay stream"
        }
        (RelayDirection::ServerToClient, RelayOperation::Flush) => {
            "flush server-to-client relay stream"
        }
        (RelayDirection::ServerToClient, RelayOperation::HalfClose) => {
            "half-close server-to-client relay stream"
        }
    }
}

pub(crate) async fn timeout_cancel_first<F, T>(
    duration: Duration,
    cancellation: &CancellationToken,
    future: F,
    timeout_code: ErrorCode,
    cancellation_message: &'static str,
    timeout_stage: &'static str,
) -> Result<T>
where
    F: Future<Output = T>,
{
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(ProxyError::new(ErrorCode::ProxyStopped, cancellation_message)),
        outcome = tokio::time::timeout(duration, future) => outcome.map_err(|_| ProxyError::new(
            timeout_code,
            format!("{timeout_stage} timed out after {} ms", duration.as_millis()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

    use super::*;

    #[tokio::test]
    async fn relay_preserves_binary_large_buffers_and_directional_counts() {
        let payload = (0..(RELAY_BUFFER_BYTES * 2 + 73))
            .map(|index| u8::try_from(index % 251).unwrap())
            .collect::<Vec<_>>();
        let reply = vec![0, 255, 0, 128, 42];
        let (mut client, relay_downstream) = tokio::io::duplex(payload.len() * 2);
        let (relay_upstream, mut server) = tokio::io::duplex(payload.len() * 2);
        let relay = tokio::spawn(relay_bidirectional(
            Box::new(relay_downstream),
            Box::new(relay_upstream),
            RelayTimeouts::new(
                Duration::from_secs(1),
                Duration::from_secs(1),
                RelayTimeoutCodes::upstream(),
            ),
            CancellationToken::new(),
        ));

        client.write_all(&payload).await.unwrap();
        client.shutdown().await.unwrap();
        let mut received = Vec::new();
        server.read_to_end(&mut received).await.unwrap();
        assert_eq!(received, payload);
        server.write_all(&reply).await.unwrap();
        server.shutdown().await.unwrap();
        let mut actual_reply = Vec::new();
        client.read_to_end(&mut actual_reply).await.unwrap();

        assert_eq!(actual_reply, reply);
        assert_eq!(
            relay.await.unwrap().unwrap(),
            RelayBytes {
                client_to_server: payload.len() as u64,
                server_to_client: reply.len() as u64,
            }
        );
    }

    #[tokio::test]
    async fn downstream_half_close_does_not_discard_the_later_reply() {
        let (mut client, relay_downstream) = tokio::io::duplex(1024);
        let (relay_upstream, mut server) = tokio::io::duplex(1024);
        let relay = tokio::spawn(relay_bidirectional(
            Box::new(relay_downstream),
            Box::new(relay_upstream),
            RelayTimeouts::new(
                Duration::from_secs(1),
                Duration::from_secs(1),
                RelayTimeoutCodes::upstream(),
            ),
            CancellationToken::new(),
        ));

        client.write_all(b"request").await.unwrap();
        client.shutdown().await.unwrap();
        let mut request = Vec::new();
        server.read_to_end(&mut request).await.unwrap();
        server.write_all(b"reply").await.unwrap();
        server.shutdown().await.unwrap();
        let mut reply = Vec::new();
        client.read_to_end(&mut reply).await.unwrap();

        assert_eq!(request, b"request");
        assert_eq!(reply, b"reply");
        assert!(relay.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn pre_cancelled_operation_wins_over_an_immediately_ready_future() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let error = timeout_cancel_first(
            Duration::ZERO,
            &cancellation,
            std::future::ready(7),
            ErrorCode::UpstreamReadTimeout,
            "cancelled first",
            "test stage",
        )
        .await
        .expect_err("cancellation must win");

        assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
        assert_eq!(error.message, "cancelled first");
    }

    #[tokio::test]
    async fn silent_read_reports_direction_and_read_timeout() {
        let failure = copy_direction(
            PendingIo,
            tokio::io::sink(),
            RelayDirection::ServerToClient,
            RelayTimeouts::new(
                Duration::from_millis(5),
                Duration::from_secs(1),
                RelayTimeoutCodes::upstream(),
            ),
            Arc::new(RelayProgress::default()),
            CancellationToken::new(),
        )
        .await
        .expect_err("pending read must time out");

        assert_eq!(failure.direction, RelayDirection::ServerToClient);
        assert_eq!(failure.operation, RelayOperation::Read);
        assert_eq!(failure.error.code, ErrorCode::UpstreamReadTimeout.as_str());
    }

    #[tokio::test]
    async fn partial_write_count_survives_a_write_timeout() {
        let counters = Arc::new(RelayProgress::default());
        let failure = copy_direction(
            tokio::io::repeat(0x5a).take(32),
            PartialThenPendingIo { remaining: 7 },
            RelayDirection::ClientToServer,
            RelayTimeouts::new(
                Duration::from_secs(1),
                Duration::from_millis(5),
                RelayTimeoutCodes::upstream(),
            ),
            Arc::clone(&counters),
            CancellationToken::new(),
        )
        .await
        .expect_err("second write must time out");

        assert_eq!(failure.operation, RelayOperation::Write);
        assert_eq!(failure.bytes.client_to_server, 7);
        assert_eq!(failure.error.code, ErrorCode::UpstreamWriteTimeout.as_str());
    }

    struct PendingIo;

    impl AsyncRead for PendingIo {
        fn poll_read(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            _buffer: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Pending
        }
    }

    struct PartialThenPendingIo {
        remaining: usize,
    }

    impl AsyncWrite for PartialThenPendingIo {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            buffer: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            if self.remaining == 0 {
                return Poll::Pending;
            }
            let written = self.remaining.min(buffer.len());
            self.remaining -= written;
            Poll::Ready(Ok(written))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
}
