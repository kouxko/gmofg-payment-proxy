use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, Method};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::*;

#[tokio::test]
async fn request_flush_is_confirmed_before_a_delayed_response() {
    let (client, mut origin) = tokio::io::duplex(4 * 1024);
    let (request_seen, request_seen_rx) = oneshot::channel();
    let (release_response, release_response_rx) = oneshot::channel();
    let origin_task = tokio::spawn(async move {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 512];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = origin.read(&mut buffer).await.unwrap();
            assert_ne!(read, 0, "request closed before its complete head");
            request.extend_from_slice(&buffer[..read]);
        }
        request_seen.send(()).unwrap();
        release_response_rx.await.unwrap();
        origin
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .await
            .unwrap();
    });

    let uri = http::Uri::from_static("/commit");
    let request = ForwardRequest {
        method: Method::POST,
        uri: uri.clone(),
        message: Message::request(&Method::POST, &uri, &HeaderMap::new(), Bytes::new()),
    };
    let (receipt, completion) = upstream_write_receipt();
    let exchange_task = tokio::spawn(async move {
        send_http1_request(
            Box::new(client),
            request,
            Http1ExchangeConfig {
                schedule: TrafficSchedule::default(),
                write_timeout: Duration::from_secs(1),
                read_timeout: Duration::from_secs(1),
                limits: MessageLimits::default(),
            },
            None,
            &receipt,
            &CancellationToken::new(),
        )
        .await
    });

    request_seen_rx.await.unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_millis(100),
        completion.wait(&CancellationToken::new()),
    )
    .await
    .expect("request flush must be confirmed while the response is delayed")
    .unwrap();
    assert_eq!(outcome, WriteOutcome::Committed(()));
    assert!(
        !exchange_task.is_finished(),
        "request completion must not wait for or imply response completion"
    );

    release_response.send(()).unwrap();
    let exchange = exchange_task.await.unwrap().unwrap();
    assert_eq!(exchange.final_response.http_status(), Some(200));
    assert_eq!(exchange.final_response.body, Bytes::from_static(b"ok"));
    origin_task.await.unwrap();
}

#[tokio::test]
async fn failed_flush_does_not_confirm_the_request() {
    let body = Bytes::from_static(b"request body");
    let mut headers = HeaderMap::new();
    headers.insert("host", http::HeaderValue::from_static("upstream.test"));
    headers.insert(
        "content-length",
        http::HeaderValue::from_str(&body.len().to_string()).unwrap(),
    );
    let uri = http::Uri::from_static("/resource");
    let request = ForwardRequest {
        method: Method::POST,
        uri: uri.clone(),
        message: Message::request(&Method::POST, &uri, &headers, body),
    };
    let (receipt, _completion) = upstream_write_receipt();

    let error = send_http1_request(
        Box::new(PendingWriteIo(PendingWriteStage::Flush)),
        request,
        Http1ExchangeConfig {
            schedule: TrafficSchedule::default(),
            write_timeout: Duration::from_millis(5),
            read_timeout: Duration::from_secs(1),
            limits: MessageLimits::default(),
        },
        None,
        &receipt,
        &CancellationToken::new(),
    )
    .await
    .expect_err("a failed flush must retain write-stage failure");

    assert_eq!(error.code, ErrorCode::UpstreamWriteTimeout.as_str());
    assert!(
        receipt.is_pending(),
        "failed flush must not confirm request"
    );
}

#[tokio::test]
async fn cancellation_and_receipt_drop_release_completion_waiters() {
    let (_receipt, completion) = upstream_write_receipt();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = completion.wait(&cancellation).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());

    let (receipt, completion) = upstream_write_receipt();
    drop(receipt);
    let error = completion
        .wait(&CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Internal.as_str());
    assert_eq!(
        error.message,
        "upstream write receipt dropped before completion"
    );
}

#[tokio::test]
async fn early_response_is_read_ready_without_confirming_request_flush() {
    let uri = http::Uri::from_static("/early");
    let request = ForwardRequest {
        method: Method::POST,
        uri: uri.clone(),
        message: Message::request(&Method::POST, &uri, &HeaderMap::new(), Bytes::new()),
    };
    let (receipt, completion) = upstream_write_receipt();

    let exchange = tokio::time::timeout(
        Duration::from_secs(1),
        send_http1_request(
            Box::new(EarlyResponseBeforeFlushIo::new()),
            request,
            Http1ExchangeConfig {
                schedule: TrafficSchedule::default(),
                write_timeout: Duration::from_secs(1),
                read_timeout: Duration::from_secs(1),
                limits: MessageLimits::default(),
            },
            None,
            &receipt,
            &CancellationToken::new(),
        ),
    )
    .await
    .expect("early response must remain readable")
    .unwrap();

    assert_eq!(exchange.final_response.http_status(), Some(200));
    assert!(
        receipt.is_pending(),
        "early response must not claim an unobserved request flush"
    );
    receipt.response_ready();
    assert_eq!(
        completion.wait(&CancellationToken::new()).await.unwrap(),
        WriteOutcome::ReadReady(())
    );
}

struct EarlyResponseBeforeFlushIo {
    response: &'static [u8],
    wrote_request: bool,
}

impl EarlyResponseBeforeFlushIo {
    fn new() -> Self {
        Self {
            response: b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
            wrote_request: false,
        }
    }
}

impl AsyncRead for EarlyResponseBeforeFlushIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.wrote_request || self.response.is_empty() {
            return Poll::Pending;
        }
        let read = self.response.len().min(buffer.remaining());
        buffer.put_slice(&self.response[..read]);
        self.response = &self.response[read..];
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for EarlyResponseBeforeFlushIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.wrote_request = true;
        context.waker().wake_by_ref();
        Poll::Ready(Ok(buffer.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Pending
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
