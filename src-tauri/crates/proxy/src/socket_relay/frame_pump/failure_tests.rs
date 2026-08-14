use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::*;

fn identity() -> SocketConnectionIdentity {
    SocketConnectionIdentity {
        runtime_epoch: Uuid::nil(),
        connection_id: Uuid::from_u128(2),
        peer_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 4321)),
    }
}

fn limits() -> SocketFramePumpLimits {
    SocketFramePumpLimits::new(16, 16, 4, Duration::from_secs(1)).unwrap()
}

fn timeouts() -> SocketFramePumpTimeouts {
    SocketFramePumpTimeouts::new(Duration::from_secs(1), Duration::from_secs(1))
}

struct Factory {
    processor: std::sync::Mutex<Option<Box<dyn SocketFrameProcessor>>>,
}

impl Factory {
    fn new(processor: Box<dyn SocketFrameProcessor>) -> Self {
        Self {
            processor: std::sync::Mutex::new(Some(processor)),
        }
    }
}

impl LocalResponderProcessorFactory for Factory {
    fn create_exchange(
        &self,
        _connection: SocketConnectionIdentity,
    ) -> Box<dyn SocketFrameProcessor> {
        self.processor.lock().unwrap().take().unwrap()
    }
}

struct PanicFactory;

impl LocalResponderProcessorFactory for PanicFactory {
    fn create_exchange(
        &self,
        _connection: SocketConnectionIdentity,
    ) -> Box<dyn SocketFrameProcessor> {
        panic!("factory panic must be isolated")
    }
}

struct FixedProcessor {
    boundary: FrameBoundary,
    output: Bytes,
    panic_in_inspect: bool,
}

struct CancellingProcessor(CancellationToken);

#[async_trait]
impl SocketFrameProcessor for CancellingProcessor {
    async fn inspect(
        &mut self,
        _buffered: Bytes,
    ) -> Result<FrameBoundary, SocketProcessingFailure> {
        Ok(FrameBoundary::Complete { bytes: 1 })
    }

    async fn process(&mut self, _origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        self.0.cancel();
        Ok(Bytes::from_static(b"blocked"))
    }
}

#[async_trait]
impl SocketFrameProcessor for FixedProcessor {
    async fn inspect(
        &mut self,
        _buffered: Bytes,
    ) -> Result<FrameBoundary, SocketProcessingFailure> {
        assert!(!self.panic_in_inspect, "inspect panic must be isolated");
        Ok(self.boundary.clone())
    }

    async fn process(&mut self, _origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        Ok(self.output.clone())
    }
}

async fn local_failure(processor: FixedProcessor) -> SocketProcessingFailure {
    let (mut app, pump_io) = tokio::io::duplex(32);
    let factory = Factory::new(Box::new(processor));
    let task = tokio::spawn(async move {
        respond_framed_locally(
            Box::new(pump_io),
            identity(),
            &factory,
            limits(),
            timeouts(),
            CancellationToken::new(),
            Arc::new(RelayProgress::default()),
        )
        .await
    });
    app.write_all(&[1]).await.unwrap();
    app.shutdown().await.unwrap();
    task.await.unwrap().unwrap_err()
}

#[tokio::test]
async fn invalid_zero_and_impossible_boundaries_are_rejected() {
    for boundary in [
        FrameBoundary::Complete { bytes: 0 },
        FrameBoundary::Complete { bytes: 2 },
        FrameBoundary::NeedMore { total: 0 },
        FrameBoundary::NeedMore { total: 1 },
    ] {
        let failure = local_failure(FixedProcessor {
            boundary,
            output: Bytes::from_static(b"x"),
            panic_in_inspect: false,
        })
        .await;
        assert_eq!(
            failure.kind,
            SocketProcessingFailureKind::InvalidFrameBoundary
        );
    }
}

#[tokio::test]
async fn declared_frame_above_buffer_limit_is_rejected() {
    let failure = local_failure(FixedProcessor {
        boundary: FrameBoundary::NeedMore { total: 17 },
        output: Bytes::from_static(b"x"),
        panic_in_inspect: false,
    })
    .await;
    assert_eq!(
        failure.kind,
        SocketProcessingFailureKind::BufferLimitExceeded
    );
}

#[tokio::test]
async fn inspect_panic_becomes_typed_connection_failure() {
    let failure = local_failure(FixedProcessor {
        boundary: FrameBoundary::Complete { bytes: 1 },
        output: Bytes::from_static(b"x"),
        panic_in_inspect: true,
    })
    .await;
    assert_eq!(failure.kind, SocketProcessingFailureKind::ProcessorPanicked);
}

#[tokio::test]
async fn factory_panic_becomes_typed_connection_failure_before_any_write() {
    let (_app, pump_io) = tokio::io::duplex(32);
    let failure = respond_framed_locally(
        Box::new(pump_io),
        identity(),
        &PanicFactory,
        limits(),
        timeouts(),
        CancellationToken::new(),
        Arc::new(RelayProgress::default()),
    )
    .await
    .unwrap_err();
    assert_eq!(failure.kind, SocketProcessingFailureKind::ProcessorPanicked);
    assert_eq!(
        failure.direction,
        Some(SocketPayloadDirection::LocalExchange)
    );
    assert_eq!(failure.bytes(), RelayBytes::default());
}

#[tokio::test]
async fn cancellation_triggered_by_processing_is_rechecked_before_writing() {
    let (mut app, pump_io) = tokio::io::duplex(64);
    let cancellation = CancellationToken::new();
    let factory = Factory::new(Box::new(CancellingProcessor(cancellation.clone())));
    let task = tokio::spawn(async move {
        respond_framed_locally(
            Box::new(pump_io),
            identity(),
            &factory,
            limits(),
            timeouts(),
            cancellation,
            Arc::new(RelayProgress::default()),
        )
        .await
    });
    app.write_all(&[1]).await.unwrap();
    app.shutdown().await.unwrap();
    let mut output = Vec::new();
    app.read_to_end(&mut output).await.unwrap();
    let failure = task.await.unwrap().unwrap_err();
    assert!(output.is_empty());
    assert_eq!(failure.kind, SocketProcessingFailureKind::Cancelled);
    assert_eq!(failure.bytes(), RelayBytes::default());
}

#[tokio::test]
async fn cancellation_during_writing_allows_exactly_one_complete_output() {
    let (mut app, pump_io) = tokio::io::duplex(1);
    let factory = Factory::new(Box::new(FixedProcessor {
        boundary: FrameBoundary::Complete { bytes: 1 },
        output: Bytes::from_static(b"response"),
        panic_in_inspect: false,
    }));
    let cancellation = CancellationToken::new();
    let task_cancel = cancellation.clone();
    let progress = Arc::new(RelayProgress::default());
    let observed = Arc::clone(&progress);
    let task = tokio::spawn(async move {
        respond_framed_locally(
            Box::new(pump_io),
            identity(),
            &factory,
            limits(),
            timeouts(),
            task_cancel,
            progress,
        )
        .await
    });
    app.write_all(&[1]).await.unwrap();
    while observed.snapshot().server_to_client == 0 {
        tokio::task::yield_now().await;
    }
    cancellation.cancel();
    let mut output = [0_u8; 8];
    app.read_exact(&mut output).await.unwrap();
    assert_eq!(&output, b"response");
    assert_eq!(
        task.await.unwrap().unwrap_err().kind,
        SocketProcessingFailureKind::Cancelled
    );
    assert_eq!(observed.snapshot().server_to_client, 8);
}

struct PartialThenError {
    first_write: bool,
    read_once: bool,
}

impl AsyncRead for PartialThenError {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.read_once {
            return Poll::Ready(Ok(()));
        }
        self.read_once = true;
        buffer.put_slice(&[1]);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for PartialThenError {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.first_write {
            self.first_write = false;
            Poll::Ready(Ok(buffer.len().min(3)))
        } else {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "test reset",
            )))
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn partial_reset_preserves_successful_byte_count() {
    let progress = Arc::new(RelayProgress::default());
    let factory = Factory::new(Box::new(FixedProcessor {
        boundary: FrameBoundary::Complete { bytes: 1 },
        output: Bytes::from_static(b"response"),
        panic_in_inspect: false,
    }));
    let failure = respond_framed_locally(
        Box::new(PartialThenError {
            first_write: true,
            read_once: false,
        }),
        identity(),
        &factory,
        limits(),
        timeouts(),
        CancellationToken::new(),
        Arc::clone(&progress),
    )
    .await
    .unwrap_err();
    assert_eq!(failure.kind, SocketProcessingFailureKind::WriteFailed);
    assert_eq!(failure.bytes(), progress.snapshot());
    assert_eq!(progress.io_snapshot().read.client_to_server, 1);
    assert_eq!(failure.bytes().server_to_client, 3);
}

#[test]
fn failure_debug_never_contains_frame_payload() {
    let failure = SocketProcessingFailure::new(
        SocketProcessingFailureKind::ProcessingFailed,
        "secret-frame and sensitive processor reason",
    );
    let debug = format!("{failure:?}");
    assert!(debug.contains("ProcessingFailed"));
    assert!(!debug.contains("secret-frame"));
    assert!(!debug.contains("sensitive processor reason"));
}
