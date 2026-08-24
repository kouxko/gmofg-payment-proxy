use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use tokio::io::AsyncWrite;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

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
    assert!(error.message.contains(ErrorCode::ProxyStopped.as_str()));
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
    assert!(error.message.contains(ErrorCode::ProxyStopped.as_str()));
    let failure = failure.lock().unwrap().take().unwrap();
    assert_eq!(failure.error.code, ErrorCode::ProxyStopped.as_str());
    assert_eq!(failure.operation, RelayOperation::HalfClose);
    assert_eq!(failure.bytes, RelayBytes::default());
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
