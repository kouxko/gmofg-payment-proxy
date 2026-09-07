//! Socket 协议 Connection 的 transport Reader/Writer。
//!
//! timeout 与 cancellation 属于 Listener transport 配置，因此在这里约束真实 read、
//! write、flush、shutdown；Exchange 核心不定义默认值，也不参与重试或部分写建模。

use std::{marker::PhantomData, sync::Arc, time::Duration};

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Connection, Direction, Error, Reader, Socket, SocketContext, WriteOutcome, Writer,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

use crate::socket_relay::io as socket_io;
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
        socket_io::read(
            &mut self.reader,
            self.read_chunk_bytes,
            self.timeout,
            &self.cancellation,
            self.direction,
            &self.progress,
        )
        .await
        .map(|bytes| bytes.map(|data| SocketContext { data }))
        .map_err(|failure| socket_io::exchange_error(&failure))
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
        socket_io::write_all_and_flush(
            &mut self.writer,
            payload,
            self.timeout,
            &self.cancellation,
            self.direction,
            &self.progress,
        )
        .await
        .map_err(|failure| socket_io::exchange_error(&failure))
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        socket_io::shutdown(
            &mut self.writer,
            self.timeout,
            &self.cancellation,
            self.direction,
            &self.progress,
        )
        .await
        .map_err(|failure| socket_io::exchange_error(&failure))
    }
}

#[async_trait]
impl<D: Direction> Writer<Socket, D> for SocketWriter<D> {
    async fn write(
        &mut self,
        context: SocketContext,
    ) -> Result<WriteOutcome<SocketContext>, Error> {
        if let Err(error) = self.write_all(&context.data).await {
            tracing::error!(
                target: "intercept_proxy::exchange::diagnostic",
                direction = ?D::KIND,
                error = %error,
                "Socket transport write failed"
            );
            return Err(error);
        }
        Ok(WriteOutcome::Committed(context))
    }
}
