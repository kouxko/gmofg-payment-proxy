//! Socket 协议 Connection 的 transport Reader/Writer。
//!
//! timeout 与 cancellation 属于 Listener transport 配置，因此在这里约束真实 read、
//! write、flush、shutdown；Exchange 核心不定义默认值，也不参与重试或部分写建模。

use std::{marker::PhantomData, sync::Arc, time::Duration};

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Connection, Direction, Error, Reader, Socket, SocketContext, Writer,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::transport::relay::{RelayDirection, RelayProgress};

pub(super) struct SocketConnection<RD: Direction, WD: Direction> {
    reader: SocketReader<RD>,
    writer: SocketWriter<WD>,
}

impl<RD: Direction, WD: Direction> SocketConnection<RD, WD> {
    pub(super) fn new(reader: SocketReader<RD>, writer: SocketWriter<WD>) -> Self {
        Self { reader, writer }
    }
}

#[async_trait]
impl<RD: Direction, WD: Direction> Connection<Socket, RD, WD> for SocketConnection<RD, WD> {
    fn reader(&mut self) -> &mut dyn Reader<Socket, RD> {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut dyn Writer<Socket, WD> {
        &mut self.writer
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        self.writer.shutdown().await
    }
}

pub(super) struct SocketReader<D: Direction> {
    reader: Box<dyn AsyncRead + Send + Unpin>,
    read_chunk_bytes: usize,
    timeout: Duration,
    cancellation: CancellationToken,
    direction: RelayDirection,
    progress: Arc<RelayProgress>,
    marker: PhantomData<fn() -> D>,
}

impl<D: Direction> SocketReader<D> {
    pub(super) fn new(
        reader: Box<dyn AsyncRead + Send + Unpin>,
        read_chunk_bytes: usize,
        timeout: Duration,
        cancellation: CancellationToken,
        direction: RelayDirection,
        progress: Arc<RelayProgress>,
    ) -> Self {
        Self {
            reader,
            read_chunk_bytes,
            timeout,
            cancellation,
            direction,
            progress,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Reader<Socket, D> for SocketReader<D> {
    async fn read(&mut self) -> Result<Option<SocketContext>, Error> {
        let mut buffer = vec![0_u8; self.read_chunk_bytes];
        let result = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => return Err(direction_error::<D>(
                "CANCELLED", "socket Exchange cancelled while reading",
            )),
            result = tokio::time::timeout(self.timeout, self.reader.read(&mut buffer)) => result,
        };
        let read = result
            .map_err(|_| direction_error::<D>("READ_TIMEOUT", "socket Exchange read timed out"))?
            .map_err(|error| {
                direction_error::<D>(
                    "READ_FAILED",
                    format!("socket Exchange read failed: {error}"),
                )
            })?;
        if read == 0 {
            return Ok(None);
        }
        buffer.truncate(read);
        self.progress.add_read(self.direction, read);
        Ok(Some(SocketContext { data: buffer }))
    }
}

pub(super) struct SocketWriter<D: Direction> {
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    timeout: Duration,
    cancellation: CancellationToken,
    direction: RelayDirection,
    progress: Arc<RelayProgress>,
    marker: PhantomData<fn() -> D>,
}

impl<D: Direction> SocketWriter<D> {
    pub(super) fn new(
        writer: Box<dyn AsyncWrite + Send + Unpin>,
        timeout: Duration,
        cancellation: CancellationToken,
        direction: RelayDirection,
        progress: Arc<RelayProgress>,
    ) -> Self {
        Self {
            writer,
            timeout,
            cancellation,
            direction,
            progress,
            marker: PhantomData,
        }
    }

    async fn write_all(&mut self, payload: &[u8]) -> Result<(), Error> {
        let mut offset = 0;
        while offset < payload.len() {
            let result = tokio::select! {
                biased;
                () = self.cancellation.cancelled() => return Err(direction_error::<D>(
                    "CANCELLED", "socket Exchange cancelled while writing",
                )),
                result = tokio::time::timeout(self.timeout, self.writer.write(&payload[offset..])) => result,
            };
            let written = result
                .map_err(|_| {
                    direction_error::<D>("WRITE_TIMEOUT", "socket Exchange write timed out")
                })?
                .map_err(|error| {
                    direction_error::<D>(
                        "WRITE_FAILED",
                        format!("socket Exchange write failed: {error}"),
                    )
                })?;
            if written == 0 {
                return Err(direction_error::<D>(
                    "WRITE_FAILED",
                    "socket Exchange write returned zero",
                ));
            }
            offset += written;
            self.progress.add(self.direction, written);
        }
        tokio::select! {
            biased;
            () = self.cancellation.cancelled() => Err(direction_error::<D>(
                "CANCELLED", "socket Exchange cancelled while flushing",
            )),
            result = tokio::time::timeout(self.timeout, self.writer.flush()) => result
                .map_err(|_| direction_error::<D>("WRITE_TIMEOUT", "socket Exchange flush timed out"))?
                .map_err(|error| direction_error::<D>("WRITE_FAILED", format!("socket Exchange flush failed: {error}"))),
        }
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        tokio::time::timeout(self.timeout, self.writer.shutdown())
            .await
            .map_err(|_| {
                direction_error::<D>("WRITE_TIMEOUT", "socket Exchange shutdown timed out")
            })?
            .map_err(|error| {
                direction_error::<D>(
                    "WRITE_FAILED",
                    format!("socket Exchange shutdown failed: {error}"),
                )
            })
    }
}

#[async_trait]
impl<D: Direction> Writer<Socket, D> for SocketWriter<D> {
    async fn write(&mut self, context: SocketContext) -> Result<SocketContext, Error> {
        if let Err(error) = self.write_all(&context.data).await {
            tracing::error!(
                target: "intercept_proxy::exchange::diagnostic",
                direction = ?D::KIND,
                error = %error,
                "Socket transport write failed"
            );
            return Err(error);
        }
        Ok(context)
    }
}

fn direction_error<D: Direction>(code: &str, message: impl std::fmt::Display) -> Error {
    Error::new(format!("{:?}|{code}: {message}", D::KIND))
}
