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
    let counters = Arc::new(RelayProgress::default());
    let observed = Arc::clone(&counters);
    let relay = tokio::spawn(relay_bidirectional_with_progress(
        Box::new(relay_downstream),
        Box::new(relay_upstream),
        RelayTimeouts::new(
            Duration::from_secs(1),
            Duration::from_secs(1),
            RelayTimeoutCodes::upstream(),
        ),
        CancellationToken::new(),
        counters,
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
    let expected = RelayBytes {
        client_to_server: payload.len() as u64,
        server_to_client: reply.len() as u64,
    };
    assert_eq!(relay.await.unwrap().unwrap(), expected);
    assert_eq!(
        observed.io_snapshot(),
        RelayIoBytes {
            read: expected,
            written: expected,
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
    assert_eq!(counters.io_snapshot().read.client_to_server, 32);
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
