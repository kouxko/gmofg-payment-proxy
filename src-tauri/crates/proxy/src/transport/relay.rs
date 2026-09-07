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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RelayIoBytes {
    pub(crate) read: RelayBytes,
    pub(crate) written: RelayBytes,
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
    client_to_server_read: AtomicU64,
    server_to_client_read: AtomicU64,
    client_to_server_written: AtomicU64,
    server_to_client_written: AtomicU64,
}

impl RelayProgress {
    pub(crate) fn add_read(&self, direction: RelayDirection, bytes: usize) {
        let counter = match direction {
            RelayDirection::ClientToServer => &self.client_to_server_read,
            RelayDirection::ServerToClient => &self.server_to_client_read,
        };
        counter.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub(crate) fn add(&self, direction: RelayDirection, bytes: usize) {
        let counter = match direction {
            RelayDirection::ClientToServer => &self.client_to_server_written,
            RelayDirection::ServerToClient => &self.server_to_client_written,
        };
        counter.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> RelayBytes {
        RelayBytes {
            client_to_server: self.client_to_server_written.load(Ordering::Relaxed),
            server_to_client: self.server_to_client_written.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn io_snapshot(&self) -> RelayIoBytes {
        RelayIoBytes {
            read: RelayBytes {
                client_to_server: self.client_to_server_read.load(Ordering::Relaxed),
                server_to_client: self.server_to_client_read.load(Ordering::Relaxed),
            },
            written: self.snapshot(),
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
        counters.add_read(direction, read);

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
#[path = "relay_tests.rs"]
mod tests;
