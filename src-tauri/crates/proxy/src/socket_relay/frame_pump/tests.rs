use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::*;

fn identity() -> SocketConnectionIdentity {
    SocketConnectionIdentity {
        runtime_epoch: Uuid::nil(),
        connection_id: Uuid::from_u128(1),
        peer_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 1234)),
    }
}

fn limits() -> SocketFramePumpLimits {
    SocketFramePumpLimits::new(64, 64, 3, Duration::from_secs(1)).unwrap()
}

fn timeouts() -> SocketFramePumpTimeouts {
    SocketFramePumpTimeouts::new(Duration::from_secs(1), Duration::from_secs(1))
}

#[derive(Clone)]
struct QueueFactory {
    processors: Arc<Mutex<VecDeque<Box<dyn SocketFrameProcessor>>>>,
}

impl QueueFactory {
    fn new(processors: Vec<Box<dyn SocketFrameProcessor>>) -> Self {
        Self {
            processors: Arc::new(Mutex::new(processors.into())),
        }
    }

    fn take(&self) -> Box<dyn SocketFrameProcessor> {
        self.processors.lock().unwrap().pop_front().unwrap()
    }
}

impl LocalResponderProcessorFactory for QueueFactory {
    fn create_exchange(
        &self,
        _connection: SocketConnectionIdentity,
    ) -> Box<dyn SocketFrameProcessor> {
        self.take()
    }
}

impl ScriptedRelayProcessorFactory for QueueFactory {
    fn create_direction(
        &self,
        _connection: SocketConnectionIdentity,
        _direction: SocketPayloadDirection,
    ) -> Box<dyn SocketFrameProcessor> {
        self.take()
    }
}

struct LengthProcessor;

#[async_trait]
impl SocketFrameProcessor for LengthProcessor {
    async fn inspect(&mut self, buffered: Bytes) -> Result<FrameBoundary, SocketProcessingFailure> {
        let total = usize::from(buffered[0]) + 1;
        if buffered.len() < total {
            Ok(FrameBoundary::NeedMore { total })
        } else {
            Ok(FrameBoundary::Complete { bytes: total })
        }
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        Ok(origin)
    }
}

async fn run_local(
    input: &[u8],
    processor: Box<dyn SocketFrameProcessor>,
    selected_limits: SocketFramePumpLimits,
) -> (
    Vec<u8>,
    Result<RelayBytes, SocketProcessingFailure>,
    crate::transport::relay::RelayIoBytes,
) {
    let (mut app, pump_io) = tokio::io::duplex(256);
    let factory = QueueFactory::new(vec![processor]);
    let progress = Arc::new(RelayProgress::default());
    let observed = Arc::clone(&progress);
    let task = tokio::spawn(async move {
        respond_framed_locally(
            Box::new(pump_io),
            identity(),
            &factory,
            selected_limits,
            timeouts(),
            CancellationToken::new(),
            progress,
        )
        .await
    });
    app.write_all(input).await.unwrap();
    app.shutdown().await.unwrap();
    let mut output = Vec::new();
    app.read_to_end(&mut output).await.unwrap();
    (output, task.await.unwrap(), observed.io_snapshot())
}

#[test]
fn limits_are_strictly_validated() {
    for invalid in [
        SocketFramePumpLimits::new(0, 1, 1, Duration::from_secs(1)),
        SocketFramePumpLimits::new(1, 0, 1, Duration::from_secs(1)),
        SocketFramePumpLimits::new(1, 1, 2, Duration::from_secs(1)),
        SocketFramePumpLimits::new(1, 1, 1, Duration::ZERO),
    ] {
        assert_eq!(
            invalid.unwrap_err().kind,
            SocketProcessingFailureKind::InvalidLimits
        );
    }
}

#[tokio::test]
async fn local_handles_chunked_and_sticky_frames_in_fifo_order() {
    let input = [2, b'a', b'b', 1, b'c', 2, b'd', b'e'];
    let (output, result, bytes) = run_local(&input, Box::new(LengthProcessor), limits()).await;
    assert_eq!(output, input);
    assert_eq!(result.unwrap().server_to_client, input.len() as u64);
    assert_eq!(bytes.read.client_to_server, input.len() as u64);
}

#[tokio::test]
async fn complete_frame_is_written_before_truncated_tail_is_reported() {
    let input = [2, b'a', b'b', 3, b'x'];
    let (output, result, bytes) = run_local(&input, Box::new(LengthProcessor), limits()).await;
    assert_eq!(output, &input[..3]);
    assert_eq!(
        result.unwrap_err().kind,
        SocketProcessingFailureKind::TruncatedFrame
    );
    assert_eq!(bytes.read.client_to_server, input.len() as u64);
    assert_eq!(bytes.written.server_to_client, 3);
}

#[tokio::test]
async fn clean_eof_half_closes_local_response() {
    let (output, result, bytes) = run_local(&[], Box::new(LengthProcessor), limits()).await;
    assert!(output.is_empty());
    assert_eq!(result.unwrap(), RelayBytes::default());
    assert_eq!(bytes, crate::transport::relay::RelayIoBytes::default());
}

#[tokio::test]
async fn relay_runs_both_directions_and_preserves_half_close() {
    let (mut app, downstream) = tokio::io::duplex(128);
    let (upstream, mut server) = tokio::io::duplex(128);
    let factory = QueueFactory::new(vec![Box::new(LengthProcessor), Box::new(LengthProcessor)]);
    let task = tokio::spawn(async move {
        relay_framed_bidirectional(
            Box::new(downstream),
            Box::new(upstream),
            identity(),
            &factory,
            limits(),
            timeouts(),
            CancellationToken::new(),
            Arc::new(RelayProgress::default()),
        )
        .await
    });
    app.write_all(&[1, b'q']).await.unwrap();
    app.shutdown().await.unwrap();
    let mut request = Vec::new();
    server.read_to_end(&mut request).await.unwrap();
    assert_eq!(request, [1, b'q']);
    server.write_all(&[1, b'r']).await.unwrap();
    server.shutdown().await.unwrap();
    let mut reply = Vec::new();
    app.read_to_end(&mut reply).await.unwrap();
    assert_eq!(reply, [1, b'r']);
    assert_eq!(
        task.await.unwrap().unwrap(),
        RelayBytes {
            client_to_server: 2,
            server_to_client: 2
        }
    );
}

struct OutcomeProcessor {
    outcome: Outcome,
}

enum Outcome {
    Empty,
    Oversized,
    Error,
    Panic,
}

#[async_trait]
impl SocketFrameProcessor for OutcomeProcessor {
    async fn inspect(
        &mut self,
        _buffered: Bytes,
    ) -> Result<FrameBoundary, SocketProcessingFailure> {
        Ok(FrameBoundary::Complete { bytes: 1 })
    }

    async fn process(&mut self, _origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        match self.outcome {
            Outcome::Empty => Ok(Bytes::new()),
            Outcome::Oversized => Ok(Bytes::from(vec![7; 65])),
            Outcome::Error => Err(SocketProcessingFailure::new(
                SocketProcessingFailureKind::ProcessingFailed,
                "expected processor failure",
            )),
            Outcome::Panic => panic!("processor test panic"),
        }
    }
}

#[tokio::test]
async fn pre_write_processor_failures_emit_no_bytes() {
    let cases = [
        (Outcome::Empty, SocketProcessingFailureKind::EmptyOutput),
        (
            Outcome::Oversized,
            SocketProcessingFailureKind::OutputLimitExceeded,
        ),
        (
            Outcome::Error,
            SocketProcessingFailureKind::ProcessingFailed,
        ),
        (
            Outcome::Panic,
            SocketProcessingFailureKind::ProcessorPanicked,
        ),
    ];
    for (outcome, expected) in cases {
        let (output, result, bytes) =
            run_local(&[0], Box::new(OutcomeProcessor { outcome }), limits()).await;
        assert!(output.is_empty());
        assert_eq!(bytes.read.client_to_server, 1);
        assert_eq!(bytes.written, RelayBytes::default());
        let failure = result.unwrap_err();
        assert_eq!(failure.kind, expected);
        assert_eq!(failure.bytes(), RelayBytes::default());
    }
}

struct BlockingProcessor {
    entered: Arc<Notify>,
}

#[async_trait]
impl SocketFrameProcessor for BlockingProcessor {
    async fn inspect(
        &mut self,
        _buffered: Bytes,
    ) -> Result<FrameBoundary, SocketProcessingFailure> {
        Ok(FrameBoundary::Complete { bytes: 1 })
    }

    async fn process(&mut self, _origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        self.entered.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn processing_cancellation_discards_output() {
    let (mut app, pump_io) = tokio::io::duplex(32);
    let entered = Arc::new(Notify::new());
    let factory = QueueFactory::new(vec![Box::new(BlockingProcessor {
        entered: Arc::clone(&entered),
    })]);
    let cancellation = CancellationToken::new();
    let task_cancel = cancellation.clone();
    let task = tokio::spawn(async move {
        respond_framed_locally(
            Box::new(pump_io),
            identity(),
            &factory,
            limits(),
            timeouts(),
            task_cancel,
            Arc::new(RelayProgress::default()),
        )
        .await
    });
    app.write_all(&[0]).await.unwrap();
    entered.notified().await;
    cancellation.cancel();
    assert_eq!(
        task.await.unwrap().unwrap_err().kind,
        SocketProcessingFailureKind::Cancelled
    );
    let mut output = Vec::new();
    app.read_to_end(&mut output).await.unwrap();
    assert!(output.is_empty());
}

#[tokio::test(start_paused = true)]
async fn read_and_processing_timeouts_are_typed() {
    let (app, pump_io) = tokio::io::duplex(8);
    let factory = QueueFactory::new(vec![Box::new(LengthProcessor)]);
    let read_task = tokio::spawn(async move {
        respond_framed_locally(
            Box::new(pump_io),
            identity(),
            &factory,
            limits(),
            SocketFramePumpTimeouts::new(Duration::from_millis(10), Duration::from_secs(1)),
            CancellationToken::new(),
            Arc::new(RelayProgress::default()),
        )
        .await
    });
    tokio::time::advance(Duration::from_millis(11)).await;
    assert_eq!(
        read_task.await.unwrap().unwrap_err().kind,
        SocketProcessingFailureKind::ReadTimeout
    );
    drop(app);

    let short = SocketFramePumpLimits::new(64, 64, 3, Duration::from_millis(10)).unwrap();
    let (mut app, pump_io) = tokio::io::duplex(8);
    let factory = QueueFactory::new(vec![Box::new(BlockingProcessor {
        entered: Arc::new(Notify::new()),
    })]);
    let process_task = tokio::spawn(async move {
        respond_framed_locally(
            Box::new(pump_io),
            identity(),
            &factory,
            short,
            timeouts(),
            CancellationToken::new(),
            Arc::new(RelayProgress::default()),
        )
        .await
    });
    app.write_all(&[0]).await.unwrap();
    tokio::time::advance(Duration::from_millis(11)).await;
    assert_eq!(
        process_task.await.unwrap().unwrap_err().kind,
        SocketProcessingFailureKind::ProcessingTimeout
    );
}

#[tokio::test(start_paused = true)]
async fn bounded_write_timeout_reports_bytes_already_written() {
    let (mut app, pump_io) = tokio::io::duplex(4);
    let factory = QueueFactory::new(vec![Box::new(OutcomeProcessor {
        outcome: Outcome::Oversized,
    })]);
    let expanded = SocketFramePumpLimits::new(64, 128, 3, Duration::from_secs(1)).unwrap();
    let progress = Arc::new(RelayProgress::default());
    let observed = Arc::clone(&progress);
    let task = tokio::spawn(async move {
        respond_framed_locally(
            Box::new(pump_io),
            identity(),
            &factory,
            expanded,
            SocketFramePumpTimeouts::new(Duration::from_secs(1), Duration::from_millis(10)),
            CancellationToken::new(),
            progress,
        )
        .await
    });
    app.write_all(&[0]).await.unwrap();
    tokio::time::advance(Duration::from_millis(11)).await;
    let failure = task.await.unwrap().unwrap_err();
    assert_eq!(failure.kind, SocketProcessingFailureKind::WriteTimeout);
    assert_eq!(failure.bytes(), observed.snapshot());
    assert!(failure.bytes().server_to_client > 0);
}
