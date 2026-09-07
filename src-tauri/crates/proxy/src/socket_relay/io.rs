//! Shared Socket transport I/O with cancellation-first deadlines and exact progress.

use std::{future::Future, time::Duration};

use intercept_proxy_exchange::{DirectionKind, Error, ErrorStage};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::transport::relay::{RelayDirection, RelayFailure, RelayOperation, RelayProgress};
use crate::{ErrorCode, ProxyError};

use super::SocketProcessingFailureKind;

pub(super) async fn read<R>(
    reader: &mut R,
    read_chunk_bytes: usize,
    timeout: Duration,
    cancellation: &CancellationToken,
    direction: RelayDirection,
    progress: &RelayProgress,
) -> Result<Option<Vec<u8>>, RelayFailure>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = vec![0_u8; read_chunk_bytes];
    let read = socket_io(
        reader.read(&mut bytes),
        timeout,
        cancellation,
        ErrorCode::SocketReadTimeout,
        direction,
        RelayOperation::Read,
        progress,
    )
    .await?;
    if read == 0 {
        return Ok(None);
    }
    bytes.truncate(read);
    progress.add_read(direction, read);
    Ok(Some(bytes))
}

pub(super) async fn write_all_and_flush<W>(
    writer: &mut W,
    bytes: &[u8],
    timeout: Duration,
    cancellation: &CancellationToken,
    direction: RelayDirection,
    progress: &RelayProgress,
) -> Result<(), RelayFailure>
where
    W: AsyncWrite + Unpin,
{
    let mut offset = 0;
    while offset < bytes.len() {
        let written = socket_io(
            writer.write(&bytes[offset..]),
            timeout,
            cancellation,
            ErrorCode::SocketWriteTimeout,
            direction,
            RelayOperation::Write,
            progress,
        )
        .await?;
        if written == 0 {
            return Err(failure(
                ProxyError::new(ErrorCode::Io, "socket write returned zero"),
                direction,
                RelayOperation::Write,
                progress,
            ));
        }
        offset += written;
        progress.add(direction, written);
    }
    socket_io(
        writer.flush(),
        timeout,
        cancellation,
        ErrorCode::SocketWriteTimeout,
        direction,
        RelayOperation::Flush,
        progress,
    )
    .await
}

pub(super) async fn shutdown<W>(
    writer: &mut W,
    timeout: Duration,
    cancellation: &CancellationToken,
    direction: RelayDirection,
    progress: &RelayProgress,
) -> Result<(), RelayFailure>
where
    W: AsyncWrite + Unpin,
{
    socket_io(
        writer.shutdown(),
        timeout,
        cancellation,
        ErrorCode::SocketWriteTimeout,
        direction,
        RelayOperation::HalfClose,
        progress,
    )
    .await
}

pub(super) fn exchange_error(failure: &RelayFailure) -> Error {
    let kind = if failure.error.code == ErrorCode::ProxyStopped.as_str() {
        SocketProcessingFailureKind::Cancelled
    } else {
        match failure.operation {
            RelayOperation::Read if failure.error.code == ErrorCode::SocketReadTimeout.as_str() => {
                SocketProcessingFailureKind::ReadTimeout
            }
            RelayOperation::Read => SocketProcessingFailureKind::ReadFailed,
            RelayOperation::Write | RelayOperation::Flush | RelayOperation::HalfClose
                if failure.error.code == ErrorCode::SocketWriteTimeout.as_str() =>
            {
                SocketProcessingFailureKind::WriteTimeout
            }
            RelayOperation::Write | RelayOperation::Flush | RelayOperation::HalfClose => {
                SocketProcessingFailureKind::WriteFailed
            }
        }
    };
    Error::coded(kind.as_str(), failure.error.message.clone())
        .with_stage(error_stage(failure.operation))
        .with_direction(direction_kind(failure.direction))
}

async fn socket_io<F, T>(
    future: F,
    timeout: Duration,
    cancellation: &CancellationToken,
    timeout_code: ErrorCode,
    direction: RelayDirection,
    operation: RelayOperation,
    progress: &RelayProgress,
) -> Result<T, RelayFailure>
where
    F: Future<Output = std::io::Result<T>>,
{
    let result = tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            format!("socket {} cancelled", operation_name(operation)),
        )),
        result = tokio::time::timeout(timeout, future) => match result {
            Err(_) => Err(ProxyError::new(
                timeout_code,
                format!("socket {} timed out", operation_name(operation)),
            )),
            Ok(Err(error)) => Err(ProxyError::io(io_context(operation), &error)),
            Ok(Ok(value)) => Ok(value),
        },
    };
    result.map_err(|error| failure(error, direction, operation, progress))
}

fn failure(
    error: ProxyError,
    direction: RelayDirection,
    operation: RelayOperation,
    progress: &RelayProgress,
) -> RelayFailure {
    RelayFailure {
        error,
        direction,
        operation,
        bytes: progress.snapshot(),
    }
}

const fn error_stage(operation: RelayOperation) -> ErrorStage {
    match operation {
        RelayOperation::Read => ErrorStage::Read,
        RelayOperation::Write => ErrorStage::Write,
        RelayOperation::Flush => ErrorStage::Flush,
        RelayOperation::HalfClose => ErrorStage::Shutdown,
    }
}

const fn direction_kind(direction: RelayDirection) -> DirectionKind {
    match direction {
        RelayDirection::ClientToServer => DirectionKind::Upstream,
        RelayDirection::ServerToClient => DirectionKind::Downstream,
    }
}

const fn operation_name(operation: RelayOperation) -> &'static str {
    match operation {
        RelayOperation::Read => "read",
        RelayOperation::Write => "write",
        RelayOperation::Flush => "flush",
        RelayOperation::HalfClose => "shutdown",
    }
}

const fn io_context(operation: RelayOperation) -> &'static str {
    match operation {
        RelayOperation::Read => "socket read",
        RelayOperation::Write => "socket write",
        RelayOperation::Flush => "socket flush",
        RelayOperation::HalfClose => "socket shutdown",
    }
}
