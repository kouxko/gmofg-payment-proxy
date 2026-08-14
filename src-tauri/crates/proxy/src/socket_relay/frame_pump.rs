use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::FutureExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::transport::BoxIo;
use crate::transport::relay::{RelayBytes, RelayDirection, RelayProgress};

use super::processing::{
    FrameBoundary, LocalResponderProcessorFactory, ScriptedRelayProcessorFactory,
    SocketConnectionIdentity, SocketFrameProcessor, SocketFramePumpLimits, SocketPayloadDirection,
    SocketProcessingFailure, SocketProcessingFailureKind,
};
use control::{
    choose_relay_result, create_local_processor, create_relay_processor, pump_and_cancel,
};

mod control;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SocketFramePumpTimeouts {
    pub(crate) read: Duration,
    pub(crate) write: Duration,
}

impl SocketFramePumpTimeouts {
    pub(crate) const fn new(read: Duration, write: Duration) -> Self {
        Self { read, write }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn relay_framed_bidirectional(
    downstream: BoxIo,
    upstream: BoxIo,
    connection: SocketConnectionIdentity,
    factory: &dyn ScriptedRelayProcessorFactory,
    limits: SocketFramePumpLimits,
    timeouts: SocketFramePumpTimeouts,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
) -> Result<RelayBytes, SocketProcessingFailure> {
    let (downstream_read, downstream_write) = tokio::io::split(downstream);
    let (upstream_read, upstream_write) = tokio::io::split(upstream);
    let shared_cancel = cancellation.child_token();
    let first_failure = Arc::new(Mutex::new(None));
    let app_processor = create_relay_processor(
        factory,
        connection.clone(),
        SocketPayloadDirection::AppToUpstream,
    )?;
    let upstream_processor =
        create_relay_processor(factory, connection, SocketPayloadDirection::UpstreamToApp)?;
    let first = pump_and_cancel(
        pump_direction(
            downstream_read,
            upstream_write,
            app_processor,
            SocketPayloadDirection::AppToUpstream,
            limits,
            timeouts,
            shared_cancel.clone(),
            Arc::clone(&progress),
        ),
        shared_cancel.clone(),
        Arc::clone(&first_failure),
    );
    let second = pump_and_cancel(
        pump_direction(
            upstream_read,
            downstream_write,
            upstream_processor,
            SocketPayloadDirection::UpstreamToApp,
            limits,
            timeouts,
            shared_cancel.clone(),
            Arc::clone(&progress),
        ),
        shared_cancel,
        Arc::clone(&first_failure),
    );
    let (first, second) = tokio::join!(first, second);
    let bytes = progress.snapshot();
    let recorded = first_failure
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    recorded
        .map_or_else(|| choose_relay_result(first, second, bytes), Err)
        .map_err(|failure| failure.with_bytes(bytes))
}

pub(crate) async fn respond_framed_locally(
    app: BoxIo,
    connection: SocketConnectionIdentity,
    factory: &dyn LocalResponderProcessorFactory,
    limits: SocketFramePumpLimits,
    timeouts: SocketFramePumpTimeouts,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
) -> Result<RelayBytes, SocketProcessingFailure> {
    let (reader, writer) = tokio::io::split(app);
    let processor = create_local_processor(factory, connection)?;
    let result = pump_direction(
        reader,
        writer,
        processor,
        SocketPayloadDirection::LocalExchange,
        limits,
        timeouts,
        cancellation,
        Arc::clone(&progress),
    )
    .await;
    let bytes = progress.snapshot();
    result
        .map(|()| bytes)
        .map_err(|failure| failure.with_bytes(bytes))
}

#[allow(clippy::too_many_arguments)]
async fn pump_direction<R, W>(
    mut reader: R,
    mut writer: W,
    mut processor: Box<dyn SocketFrameProcessor>,
    direction: SocketPayloadDirection,
    limits: SocketFramePumpLimits,
    timeouts: SocketFramePumpTimeouts,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
) -> Result<(), SocketProcessingFailure>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffered = BytesMut::with_capacity(limits.read_chunk_bytes());
    let mut eof = false;
    loop {
        if buffered.is_empty() {
            if eof {
                shutdown_writer(&mut writer, direction, timeouts.write).await?;
                return Ok(());
            }
            read_more(
                &mut reader,
                &mut buffered,
                direction,
                limits,
                timeouts.read,
                &cancellation,
                &progress,
            )
            .await
            .map(|read| eof = read == 0)?;
            continue;
        }

        let boundary = run_processor_stage(
            processor.inspect(buffered.clone().freeze()),
            direction,
            limits.processing_timeout(),
            &cancellation,
        )
        .await?;
        match boundary {
            FrameBoundary::Complete { bytes } if bytes > 0 && bytes <= buffered.len() => {
                let origin = buffered.split_to(bytes).freeze();
                let output = run_processor_stage(
                    processor.process(origin),
                    direction,
                    limits.processing_timeout(),
                    &cancellation,
                )
                .await?;
                validate_output(&output, limits, direction)?;
                // 这里是 Processing -> Writing 的唯一线性化点。通过检查后，后续 write
                // 故意不再监听 cancellation；否则 cancel-safe 不完整的 AsyncWrite 可能
                // 已提交前缀后被 drop，并在上层重试时重复写同一 response。
                begin_writing(&cancellation, direction)?;
                write_output(&mut writer, &output, direction, timeouts.write, &progress).await?;
            }
            FrameBoundary::Complete { .. } | FrameBoundary::NeedMore { total: 0 } => {
                return fail(
                    SocketProcessingFailureKind::InvalidFrameBoundary,
                    direction,
                    "processor returned an invalid frame boundary",
                );
            }
            FrameBoundary::NeedMore { total } => {
                if total <= buffered.len() {
                    return fail(
                        SocketProcessingFailureKind::InvalidFrameBoundary,
                        direction,
                        "NeedMore total must exceed buffered bytes",
                    );
                }
                if total > limits.max_buffer_bytes() || buffered.len() >= limits.max_buffer_bytes()
                {
                    return fail(
                        SocketProcessingFailureKind::BufferLimitExceeded,
                        direction,
                        "frame exceeds the configured buffer limit",
                    );
                }
                if eof {
                    return fail(
                        SocketProcessingFailureKind::TruncatedFrame,
                        direction,
                        "stream ended with an incomplete frame",
                    );
                }
                read_more(
                    &mut reader,
                    &mut buffered,
                    direction,
                    limits,
                    timeouts.read,
                    &cancellation,
                    &progress,
                )
                .await
                .map(|read| eof = read == 0)?;
            }
            FrameBoundary::Reject { reason } => {
                return fail(
                    SocketProcessingFailureKind::FrameRejected,
                    direction,
                    reason,
                );
            }
        }
    }
}

fn begin_writing(
    cancellation: &CancellationToken,
    direction: SocketPayloadDirection,
) -> Result<(), SocketProcessingFailure> {
    if cancellation.is_cancelled() {
        fail(
            SocketProcessingFailureKind::Cancelled,
            direction,
            "socket frame pump cancelled before writing",
        )
    } else {
        Ok(())
    }
}

async fn read_more<R: AsyncRead + Unpin>(
    reader: &mut R,
    buffered: &mut BytesMut,
    direction: SocketPayloadDirection,
    limits: SocketFramePumpLimits,
    timeout: Duration,
    cancellation: &CancellationToken,
    progress: &RelayProgress,
) -> Result<usize, SocketProcessingFailure> {
    let available = limits.max_buffer_bytes() - buffered.len();
    if available == 0 {
        return fail(
            SocketProcessingFailureKind::BufferLimitExceeded,
            direction,
            "frame buffer is full",
        );
    }
    let chunk = available.min(limits.read_chunk_bytes());
    let mut scratch = vec![0_u8; chunk];
    let read = cancellable_timeout(
        reader.read(&mut scratch),
        timeout,
        cancellation,
        SocketProcessingFailureKind::ReadTimeout,
        direction,
    )
    .await?
    .map_err(|error| {
        SocketProcessingFailure::new(
            SocketProcessingFailureKind::ReadFailed,
            format!("socket read failed: {error}"),
        )
        .in_direction(direction)
    })?;
    buffered.extend_from_slice(&scratch[..read]);
    progress.add_read(read_relay_direction(direction), read);
    Ok(read)
}

async fn run_processor_stage<F, T>(
    future: F,
    direction: SocketPayloadDirection,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<T, SocketProcessingFailure>
where
    F: Future<Output = Result<T, SocketProcessingFailure>>,
{
    let caught = AssertUnwindSafe(future).catch_unwind();
    match cancellable_timeout(
        caught,
        timeout,
        cancellation,
        SocketProcessingFailureKind::ProcessingTimeout,
        direction,
    )
    .await?
    {
        Ok(result) => result.map_err(|failure| failure.in_direction(direction)),
        Err(_) => fail(
            SocketProcessingFailureKind::ProcessorPanicked,
            direction,
            "socket frame processor panicked",
        ),
    }
}

async fn cancellable_timeout<F, T>(
    future: F,
    duration: Duration,
    cancellation: &CancellationToken,
    timeout_kind: SocketProcessingFailureKind,
    direction: SocketPayloadDirection,
) -> Result<T, SocketProcessingFailure>
where
    F: Future<Output = T>,
{
    tokio::select! {
        biased;
        () = cancellation.cancelled() => fail(
            SocketProcessingFailureKind::Cancelled,
            direction,
            "socket frame pump cancelled",
        ),
        result = tokio::time::timeout(duration, future) => result.map_err(|_| {
            SocketProcessingFailure::new(timeout_kind, "socket frame pump stage timed out")
                .in_direction(direction)
        }),
    }
}

fn validate_output(
    output: &Bytes,
    limits: SocketFramePumpLimits,
    direction: SocketPayloadDirection,
) -> Result<(), SocketProcessingFailure> {
    if output.is_empty() {
        return fail(
            SocketProcessingFailureKind::EmptyOutput,
            direction,
            "processor returned an empty output",
        );
    }
    if output.len() > limits.max_output_bytes() {
        return fail(
            SocketProcessingFailureKind::OutputLimitExceeded,
            direction,
            "processor output exceeds the configured limit",
        );
    }
    Ok(())
}

async fn write_output<W: AsyncWrite + Unpin>(
    writer: &mut W,
    output: &[u8],
    direction: SocketPayloadDirection,
    timeout: Duration,
    progress: &RelayProgress,
) -> Result<(), SocketProcessingFailure> {
    let operation = async {
        let mut offset = 0;
        while offset < output.len() {
            let written = writer.write(&output[offset..]).await.map_err(|error| {
                SocketProcessingFailure::new(
                    SocketProcessingFailureKind::WriteFailed,
                    format!("socket write failed: {error}"),
                )
                .in_direction(direction)
            })?;
            if written == 0 {
                return fail(
                    SocketProcessingFailureKind::WriteFailed,
                    direction,
                    "socket write returned zero",
                );
            }
            offset += written;
            progress.add(relay_direction(direction), written);
        }
        writer.flush().await.map_err(|error| {
            SocketProcessingFailure::new(
                SocketProcessingFailureKind::WriteFailed,
                format!("socket flush failed: {error}"),
            )
            .in_direction(direction)
        })
    };
    tokio::time::timeout(timeout, operation)
        .await
        .map_err(|_| {
            SocketProcessingFailure::new(
                SocketProcessingFailureKind::WriteTimeout,
                "socket write timed out",
            )
            .in_direction(direction)
        })?
}

async fn shutdown_writer<W: AsyncWrite + Unpin>(
    writer: &mut W,
    direction: SocketPayloadDirection,
    timeout: Duration,
) -> Result<(), SocketProcessingFailure> {
    tokio::time::timeout(timeout, writer.shutdown())
        .await
        .map_err(|_| {
            SocketProcessingFailure::new(
                SocketProcessingFailureKind::WriteTimeout,
                "socket half-close timed out",
            )
            .in_direction(direction)
        })?
        .map_err(|error| {
            SocketProcessingFailure::new(
                SocketProcessingFailureKind::WriteFailed,
                format!("socket half-close failed: {error}"),
            )
            .in_direction(direction)
        })
}

fn relay_direction(direction: SocketPayloadDirection) -> RelayDirection {
    match direction {
        SocketPayloadDirection::AppToUpstream => RelayDirection::ClientToServer,
        SocketPayloadDirection::UpstreamToApp | SocketPayloadDirection::LocalExchange => {
            RelayDirection::ServerToClient
        }
    }
}

fn read_relay_direction(direction: SocketPayloadDirection) -> RelayDirection {
    match direction {
        SocketPayloadDirection::AppToUpstream | SocketPayloadDirection::LocalExchange => {
            RelayDirection::ClientToServer
        }
        SocketPayloadDirection::UpstreamToApp => RelayDirection::ServerToClient,
    }
}

fn fail<T>(
    kind: SocketProcessingFailureKind,
    direction: SocketPayloadDirection,
    message: impl Into<String>,
) -> Result<T, SocketProcessingFailure> {
    Err(SocketProcessingFailure::new(kind, message).in_direction(direction))
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod scheduling_tests;
#[cfg(test)]
mod tests;
