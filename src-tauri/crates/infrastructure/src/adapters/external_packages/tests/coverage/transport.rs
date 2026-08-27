use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};

use crate::adapters::external_packages::{
    ExternalPackageClient, ExternalPackageConnectionConfig, ExternalPackageConnectionError,
};

struct ScriptedIo {
    writes: usize,
    fail_write_at: Option<usize>,
    block_write_at: Option<usize>,
    fail_read: bool,
    read_bytes: &'static [u8],
}

impl ScriptedIo {
    fn fail_write_at(write: usize) -> Self {
        Self {
            writes: 0,
            fail_write_at: Some(write),
            block_write_at: None,
            fail_read: false,
            read_bytes: &[],
        }
    }

    fn fail_read() -> Self {
        Self {
            writes: 0,
            fail_write_at: None,
            block_write_at: None,
            fail_read: true,
            read_bytes: &[],
        }
    }

    fn ping_then_fail_flush() -> Self {
        // Masked client Ping containing [1, 2, 3]. The server must flush tungstenite's
        // automatically queued Pong; the second transport write fails deterministically.
        const MASKED_PING: &[u8] = &[0x89, 0x83, 1, 2, 3, 4, 0, 0, 0];
        Self {
            writes: 0,
            fail_write_at: Some(2),
            block_write_at: None,
            fail_read: false,
            read_bytes: MASKED_PING,
        }
    }

    fn block_write_at(write: usize) -> Self {
        Self {
            writes: 0,
            fail_write_at: None,
            block_write_at: Some(write),
            fail_read: false,
            read_bytes: &[],
        }
    }
}

impl AsyncRead for ScriptedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.fail_read {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "scripted read failure",
            )))
        } else if self.read_bytes.is_empty() {
            Poll::Pending
        } else {
            let bytes = self.read_bytes;
            buffer.put_slice(bytes);
            self.read_bytes = &[];
            Poll::Ready(Ok(()))
        }
    }
}

impl AsyncWrite for ScriptedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        self.writes += 1;
        if self.fail_write_at == Some(self.writes) {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "scripted write failure",
            )))
        } else if self.block_write_at == Some(self.writes) {
            Poll::Pending
        } else {
            Poll::Ready(Ok(buffer.len()))
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn config() -> ExternalPackageConnectionConfig {
    ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(5),
        Duration::from_secs(15),
        1,
        1024,
        1024,
        1024,
        128,
    )
}

async fn connect(io: ScriptedIo) -> ExternalPackageConnectionError {
    let socket = WebSocketStream::from_raw_socket(io, Role::Server, None).await;
    ExternalPackageClient::connect(socket, 31, config())
        .await
        .expect_err("scripted transport must fail")
}

#[tokio::test]
async fn registration_reports_initial_write_failure_as_transport_error() {
    assert!(matches!(
        connect(ScriptedIo::fail_write_at(1)).await,
        ExternalPackageConnectionError::Transport(_)
    ));
}

#[tokio::test]
async fn registration_reports_read_failure_as_transport_error() {
    assert!(matches!(
        connect(ScriptedIo::fail_read()).await,
        ExternalPackageConnectionError::Transport(_)
    ));
}

#[tokio::test]
async fn registration_reports_pong_flush_failure_as_transport_error() {
    assert!(matches!(
        connect(ScriptedIo::ping_then_fail_flush()).await,
        ExternalPackageConnectionError::Transport(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn registration_reports_heartbeat_write_failure_as_transport_error() {
    let connecting = tokio::spawn(connect(ScriptedIo::fail_write_at(2)));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(5)).await;

    assert!(matches!(
        connecting.await.expect("join"),
        ExternalPackageConnectionError::Transport(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn blocked_registration_heartbeat_write_obeys_the_registration_phase_deadline() {
    let connecting = tokio::spawn(connect(ScriptedIo::block_write_at(2)));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(30)).await;

    assert!(matches!(
        connecting.await.expect("join"),
        ExternalPackageConnectionError::Timeout { ref method, .. }
            if method == "package.register"
    ));
}
