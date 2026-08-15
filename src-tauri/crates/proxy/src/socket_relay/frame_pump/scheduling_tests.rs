use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::{Barrier, Notify};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::*;

fn identity() -> SocketConnectionIdentity {
    SocketConnectionIdentity {
        runtime_epoch: Uuid::nil(),
        connection_id: Uuid::from_u128(3),
        peer_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 9876)),
    }
}

fn limits() -> SocketFramePumpLimits {
    SocketFramePumpLimits::new(64, 64, 8, Duration::from_secs(1)).unwrap()
}

fn timeouts() -> SocketFramePumpTimeouts {
    SocketFramePumpTimeouts::new(Duration::from_secs(1), Duration::from_secs(1))
}

struct Factory {
    processors: Mutex<Vec<Box<dyn SocketFrameProcessor>>>,
}

impl ScriptedRelayProcessorFactory for Factory {
    fn create_direction(
        &self,
        _connection: SocketConnectionIdentity,
        _direction: SocketPayloadDirection,
    ) -> Box<dyn SocketFrameProcessor> {
        self.processors.lock().unwrap().remove(0)
    }
}

impl LocalResponderProcessorFactory for Factory {
    fn create_exchange(
        &self,
        _connection: SocketConnectionIdentity,
    ) -> Box<dyn SocketFrameProcessor> {
        self.processors.lock().unwrap().remove(0)
    }
}

struct GatedProcessor {
    calls: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl SocketFrameProcessor for GatedProcessor {
    async fn inspect(
        &mut self,
        _buffered: Bytes,
    ) -> Result<FrameBoundary, SocketProcessingFailure> {
        Ok(FrameBoundary::Complete { bytes: 1 })
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.notify_one();
            self.release.notified().await;
        }
        Ok(origin)
    }
}

#[tokio::test]
async fn slow_processing_applies_backpressure_and_preserves_fifo() {
    let calls = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let factory = Factory {
        processors: Mutex::new(vec![Box::new(GatedProcessor {
            calls: Arc::clone(&calls),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        })]),
    };
    let (mut app, pump_io) = tokio::io::duplex(32);
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
    app.write_all(b"ab").await.unwrap();
    app.shutdown().await.unwrap();
    entered.notified().await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    release.notify_one();
    let mut output = Vec::new();
    app.read_to_end(&mut output).await.unwrap();
    assert_eq!(output, b"ab");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    task.await.unwrap().unwrap();
}

struct CountingProcessor {
    processed: Arc<AtomicUsize>,
    committed: Arc<AtomicUsize>,
}

#[async_trait]
impl SocketFrameProcessor for CountingProcessor {
    async fn inspect(
        &mut self,
        _buffered: Bytes,
    ) -> Result<FrameBoundary, SocketProcessingFailure> {
        Ok(FrameBoundary::Complete { bytes: 1 })
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        self.processed.fetch_add(1, Ordering::SeqCst);
        Ok(origin)
    }

    fn output_committed(&mut self) {
        self.committed.fetch_add(1, Ordering::SeqCst);
    }
}

struct FlushGateIo {
    input_sent: bool,
    output: Arc<Mutex<Vec<u8>>>,
    flush_released: Arc<AtomicBool>,
    flush_waker: Arc<Mutex<Option<Waker>>>,
}

impl AsyncRead for FlushGateIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.input_sent {
            return Poll::Ready(Ok(()));
        }
        self.input_sent = true;
        buffer.put_slice(&[1, 2]);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for FlushGateIo {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.output.lock().unwrap().extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.flush_released.load(Ordering::SeqCst) {
            Poll::Ready(Ok(()))
        } else {
            *self.flush_waker.lock().unwrap() = Some(context.waker().clone());
            Poll::Pending
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn local_waits_for_response_flush_before_processing_the_next_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let committed = Arc::new(AtomicUsize::new(0));
    let output = Arc::new(Mutex::new(Vec::new()));
    let flush_released = Arc::new(AtomicBool::new(false));
    let flush_waker = Arc::new(Mutex::new(None));
    let factory = Factory {
        processors: Mutex::new(vec![Box::new(CountingProcessor {
            processed: Arc::clone(&calls),
            committed: Arc::clone(&committed),
        })]),
    };
    let task = tokio::spawn({
        let output = Arc::clone(&output);
        let flush_released = Arc::clone(&flush_released);
        let flush_waker = Arc::clone(&flush_waker);
        async move {
            respond_framed_locally(
                Box::new(FlushGateIo {
                    input_sent: false,
                    output,
                    flush_released,
                    flush_waker,
                }),
                identity(),
                &factory,
                limits(),
                timeouts(),
                CancellationToken::new(),
                Arc::new(RelayProgress::default()),
            )
            .await
        }
    });

    tokio::time::timeout(Duration::from_secs(1), async {
        while output.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first response must reach the flush gate");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(committed.load(Ordering::SeqCst), 0);
    assert_eq!(&*output.lock().unwrap(), &[1]);

    flush_released.store(true, Ordering::SeqCst);
    if let Some(waker) = flush_waker.lock().unwrap().take() {
        waker.wake();
    }
    task.await.unwrap().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(committed.load(Ordering::SeqCst), 2);
    assert_eq!(&*output.lock().unwrap(), &[1, 2]);
}

struct BarrierProcessor {
    barrier: Arc<Barrier>,
}

#[async_trait]
impl SocketFrameProcessor for BarrierProcessor {
    async fn inspect(
        &mut self,
        _buffered: Bytes,
    ) -> Result<FrameBoundary, SocketProcessingFailure> {
        Ok(FrameBoundary::Complete { bytes: 1 })
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        self.barrier.wait().await;
        Ok(origin)
    }
}

#[tokio::test]
async fn relay_processing_is_concurrent_across_directions() {
    let barrier = Arc::new(Barrier::new(2));
    let factory = Factory {
        processors: Mutex::new(vec![
            Box::new(BarrierProcessor {
                barrier: Arc::clone(&barrier),
            }),
            Box::new(BarrierProcessor { barrier }),
        ]),
    };
    let (mut app, downstream) = tokio::io::duplex(32);
    let (upstream, mut server) = tokio::io::duplex(32);
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
    app.write_all(b"a").await.unwrap();
    server.write_all(b"s").await.unwrap();
    let mut at_server = [0];
    let mut at_app = [0];
    server.read_exact(&mut at_server).await.unwrap();
    app.read_exact(&mut at_app).await.unwrap();
    assert_eq!(at_server, *b"a");
    assert_eq!(at_app, *b"s");
    app.shutdown().await.unwrap();
    server.shutdown().await.unwrap();
    task.await.unwrap().unwrap();
}

struct FailingProcessor;

#[async_trait]
impl SocketFrameProcessor for FailingProcessor {
    async fn inspect(
        &mut self,
        _buffered: Bytes,
    ) -> Result<FrameBoundary, SocketProcessingFailure> {
        Err(SocketProcessingFailure::new(
            SocketProcessingFailureKind::ProcessingFailed,
            "expected failure",
        ))
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        Ok(origin)
    }
}

#[tokio::test]
async fn one_direction_failure_cancels_a_silent_sibling() {
    let factory = Factory {
        processors: Mutex::new(vec![
            Box::new(FailingProcessor),
            Box::new(BarrierProcessor {
                barrier: Arc::new(Barrier::new(2)),
            }),
        ]),
    };
    let (mut app, downstream) = tokio::io::duplex(8);
    let (upstream, _server) = tokio::io::duplex(8);
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
    app.write_all(b"x").await.unwrap();
    let failure = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("silent sibling must be cancelled")
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.kind, SocketProcessingFailureKind::ProcessingFailed);
}
