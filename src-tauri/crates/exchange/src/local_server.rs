//! 与 `RemoteServer` 使用相同端口的进程内协议 Echo 和 raw Echo Endpoint。

use std::marker::PhantomData;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{
    Connection, Downstream, Error, Http, Protocol, RawConnection, RawReader, RawServer, RawWriter,
    Reader, Server, ServerConnection, Socket, Upstream, Writer,
};

/// 协议 `LocalServer` 将 upstream Writer 收到的完整 Context 原样交给 downstream Reader。
/// 它仍经过完整的 upstream/downstream Pipeline，不是旁路 responder。
pub struct LocalServer<P: Protocol> {
    marker: PhantomData<P>,
}

impl<P: Protocol> std::fmt::Debug for LocalServer<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalServer")
            .field("protocol", &std::any::type_name::<P>())
            .finish()
    }
}

impl<P: Protocol> LocalServer<P> {
    pub const fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<P: Protocol> Default for LocalServer<P> {
    fn default() -> Self {
        Self::new()
    }
}

pub type LocalHttpServer = LocalServer<Http>;
pub type LocalSocketServer = LocalServer<Socket>;

#[async_trait]
impl<P: Protocol> Server<P> for LocalServer<P> {
    async fn connect(&mut self, _first: &P::Context) -> Result<Box<ServerConnection<P>>, Error> {
        // 严格请求/回复模型只允许一条在途 Context；容量 1 提供背压而不是积累内存。
        let (sender, receiver) = mpsc::channel(1);
        Ok(Box::new(LocalProtocolConnection {
            reader: LocalProtocolReader { receiver },
            writer: LocalProtocolWriter {
                sender: Some(sender),
                marker: PhantomData,
            },
        }))
    }
}

struct LocalProtocolReader<P: Protocol> {
    receiver: mpsc::Receiver<P::Context>,
}

#[async_trait]
impl<P: Protocol> Reader<P, Downstream> for LocalProtocolReader<P> {
    async fn read(&mut self) -> Result<Option<P::Context>, Error> {
        Ok(self.receiver.recv().await)
    }
}

struct LocalProtocolWriter<P: Protocol> {
    sender: Option<mpsc::Sender<P::Context>>,
    marker: PhantomData<P>,
}

#[async_trait]
impl<P: Protocol> Writer<P, Upstream> for LocalProtocolWriter<P> {
    async fn write(&mut self, context: P::Context) -> Result<P::Context, Error> {
        let written = context.clone();
        match self.sender.as_ref() {
            Some(sender) => sender
                .send(context)
                .await
                .map_err(|_| Error::new("LocalServer reader is closed"))
                .map(|()| written),
            None => Err(Error::new("LocalServer writer is closed")),
        }
    }
}

struct LocalProtocolConnection<P: Protocol> {
    reader: LocalProtocolReader<P>,
    writer: LocalProtocolWriter<P>,
}

#[async_trait]
impl<P: Protocol> Connection<P, Downstream, Upstream> for LocalProtocolConnection<P> {
    fn reader(&mut self) -> &mut dyn Reader<P, Downstream> {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut dyn Writer<P, Upstream> {
        &mut self.writer
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        self.writer.sender.take();
        self.reader.receiver.close();
        Ok(())
    }
}

/// 透明 `LocalServer` 每次收到一个非空 raw chunk 后立即 Echo 相同字节。
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalRawServer;

impl LocalRawServer {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RawServer for LocalRawServer {
    async fn connect(&mut self, _first_app_bytes: &[u8]) -> Result<Box<dyn RawConnection>, Error> {
        // Local Endpoint 没有独立调度需求：Writer 直接把完整 chunk 交给容量 1 的
        // downstream channel。外部 TransparentExchange 是唯一 poller，因此不会产生
        // 后台 task、JoinHandle 或连接结束后仍存活的工作。Reader Drop 取消输出，Writer
        // finish/Drop 关闭 channel 并让 Reader 得到 EOF。
        let (output_sender, output_receiver) = mpsc::channel::<Vec<u8>>(1);

        Ok(Box::new(LocalRawConnection {
            reader: LocalRawReader {
                receiver: output_receiver,
            },
            writer: LocalRawWriter {
                sender: Some(output_sender),
            },
        }))
    }
}

struct LocalRawConnection {
    reader: LocalRawReader,
    writer: LocalRawWriter,
}

impl RawConnection for LocalRawConnection {
    fn into_split(self: Box<Self>) -> (Box<dyn RawReader>, Box<dyn RawWriter>) {
        (Box::new(self.reader), Box::new(self.writer))
    }
}

struct LocalRawReader {
    receiver: mpsc::Receiver<Vec<u8>>,
}

#[async_trait]
impl RawReader for LocalRawReader {
    async fn read(&mut self) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.receiver.recv().await)
    }
}

struct LocalRawWriter {
    sender: Option<mpsc::Sender<Vec<u8>>>,
}

#[async_trait]
impl RawWriter for LocalRawWriter {
    async fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.is_empty() {
            return Err(Error::new("LocalRawServer cannot write an empty chunk"));
        }
        self.sender
            .as_ref()
            .ok_or_else(|| Error::new("LocalRawServer write half is closed"))?
            .send(bytes.to_vec())
            .await
            .map_err(|_| Error::new("LocalRawServer read half is closed"))
    }

    async fn finish(&mut self) -> Result<(), Error> {
        self.sender.take();
        Ok(())
    }
}
