use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use tokio::io::AsyncWrite;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::ErrorCode;
use crate::transport::relay::RelayOperation;

use super::*;

#[tokio::test]
async fn cancellation_interrupts_a_pending_raw_flush() {
    let flush_polled = Arc::new(Notify::new());
    let cancellation = CancellationToken::new();
    let failure = Arc::new(Mutex::new(None));
    let mut writer = test_writer(
        Arc::clone(&flush_polled),
        Arc::new(Notify::new()),
        cancellation.clone(),
        Arc::clone(&failure),
    );

    let task = tokio::spawn(async move { writer.write(b"payload").await });
    flush_polled.notified().await;
    cancellation.cancel();

    let error = tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("cancellation must interrupt a pending raw flush")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code.as_deref(), Some("CANCELLED"));
    let failure = failure.lock().unwrap().take().unwrap();
    assert_eq!(failure.error.code, ErrorCode::ProxyStopped.as_str());
    assert_eq!(failure.operation, RelayOperation::Flush);
    assert_eq!(failure.bytes.client_to_server, b"payload".len() as u64);
}

#[tokio::test]
async fn cancellation_interrupts_a_pending_raw_half_close() {
    let shutdown_polled = Arc::new(Notify::new());
    let cancellation = CancellationToken::new();
    let failure = Arc::new(Mutex::new(None));
    let mut writer = test_writer(
        Arc::new(Notify::new()),
        Arc::clone(&shutdown_polled),
        cancellation.clone(),
        Arc::clone(&failure),
    );

    let task = tokio::spawn(async move { writer.finish().await });
    shutdown_polled.notified().await;
    cancellation.cancel();

    let error = tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("cancellation must interrupt a pending raw half-close")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code.as_deref(), Some("CANCELLED"));
    let failure = failure.lock().unwrap().take().unwrap();
    assert_eq!(failure.error.code, ErrorCode::ProxyStopped.as_str());
    assert_eq!(failure.operation, RelayOperation::HalfClose);
    assert_eq!(failure.bytes, RelayBytes::default());
}

#[tokio::test]
async fn partial_write_timeout_preserves_structured_error_and_progress() {
    let progress = Arc::new(RelayProgress::default());
    let failure = Arc::new(Mutex::new(None));
    let mut writer = SocketRawWriter {
        writer: PartialThenPendingWrite { remaining: 7 },
        timeout: Duration::from_millis(5),
        cancellation: CancellationToken::new(),
        progress: Arc::clone(&progress),
        failure: Arc::clone(&failure),
        direction: RelayDirection::ClientToServer,
    };

    let error = writer.write(b"0123456789").await.unwrap_err();

    assert_eq!(error.code.as_deref(), Some("WRITE_TIMEOUT"));
    assert_eq!(
        error.stage,
        Some(intercept_proxy_exchange::ErrorStage::Write)
    );
    assert_eq!(
        error.direction,
        Some(intercept_proxy_exchange::DirectionKind::Upstream)
    );
    let failure = failure.lock().unwrap().take().unwrap();
    assert_eq!(failure.operation, RelayOperation::Write);
    assert_eq!(failure.bytes.client_to_server, 7);
    assert_eq!(progress.snapshot().client_to_server, 7);
}

#[tokio::test]
async fn flush_failure_preserves_structured_error_direction_and_written_bytes() {
    let progress = Arc::new(RelayProgress::default());
    let failure = Arc::new(Mutex::new(None));
    let mut writer = SocketRawWriter {
        writer: FlushFailure,
        timeout: Duration::from_secs(1),
        cancellation: CancellationToken::new(),
        progress,
        failure: Arc::clone(&failure),
        direction: RelayDirection::ServerToClient,
    };

    let error = writer.write(b"reply").await.unwrap_err();

    assert_eq!(error.code.as_deref(), Some("WRITE_FAILED"));
    assert_eq!(
        error.stage,
        Some(intercept_proxy_exchange::ErrorStage::Flush)
    );
    assert_eq!(
        error.direction,
        Some(intercept_proxy_exchange::DirectionKind::Downstream)
    );
    let failure = failure.lock().unwrap().take().unwrap();
    assert_eq!(failure.operation, RelayOperation::Flush);
    assert_eq!(failure.bytes.server_to_client, 5);
}

fn test_writer(
    flush_polled: Arc<Notify>,
    shutdown_polled: Arc<Notify>,
    cancellation: CancellationToken,
    failure: SharedRelayFailure,
) -> SocketRawWriter<PendingTerminalWrite> {
    SocketRawWriter {
        writer: PendingTerminalWrite {
            flush_polled,
            shutdown_polled,
        },
        timeout: Duration::from_secs(30),
        cancellation,
        progress: Arc::new(RelayProgress::default()),
        failure,
        direction: RelayDirection::ClientToServer,
    }
}

struct PendingTerminalWrite {
    flush_polled: Arc<Notify>,
    shutdown_polled: Arc<Notify>,
}

struct PartialThenPendingWrite {
    remaining: usize,
}

impl AsyncWrite for PartialThenPendingWrite {
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

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct FlushFailure;

impl AsyncWrite for FlushFailure {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(buffer.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Err(std::io::Error::other("forced flush failure")))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for PendingTerminalWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(buffer.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.flush_polled.notify_one();
        Poll::Pending
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        self.shutdown_polled.notify_one();
        Poll::Pending
    }
}
