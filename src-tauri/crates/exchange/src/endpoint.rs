//! 协议连接和固定 Server Endpoint 的所有权边界。

use std::fmt;

use async_trait::async_trait;

use crate::{Direction, Downstream, Error, ObservedProtocol, Protocol, Upstream, observation};

#[async_trait]
pub trait Reader<P: Protocol, D: Direction>: Send {
    /// `Some` 是 transport 提供的数据，`None` 是该读取方向 EOF。
    async fn read(&mut self) -> Result<Option<P::Context>, Error>;
}

#[async_trait]
pub trait Writer<P: Protocol, D: Direction>: Send {
    /// 只有完整 write + flush 成功才能返回实际写出的 Context；失败不重试，也不返回
    /// committed prefix。HTTP Writer 可在 wire policy 后返回最终 Header/Body，确保 sent
    /// 证据与线上数据一致；Socket Writer 原样返回已提交字节。
    async fn write(&mut self, context: P::Context) -> Result<P::Context, Error>;
}

/// 一条全双工连接拥有 Reader、Writer 和最终完整关闭能力。
#[async_trait]
pub trait Connection<P, RD, WD>: Send
where
    P: Protocol,
    RD: Direction,
    WD: Direction,
{
    fn reader(&mut self) -> &mut dyn Reader<P, RD>;
    fn writer(&mut self) -> &mut dyn Writer<P, WD>;
    async fn shutdown(&mut self) -> Result<(), Error>;
}

pub type AppConnection<P> = dyn Connection<P, Upstream, Downstream>;
pub type ServerConnection<P> = dyn Connection<P, Downstream, Upstream>;

/// 一个 `Server` 实例就是 Exchange 创建时固定的 Local 或 Remote Endpoint。
#[async_trait]
pub trait Server<P: Protocol>: Send {
    async fn connect(&mut self, first: &P::Context) -> Result<Box<ServerConnection<P>>, Error>;
}

/// 延迟到第一条 upstream 消息才建立真实 Server connection，之后终身复用同一 Endpoint。
pub struct ServerSlot<P: Protocol> {
    server: Box<dyn Server<P>>,
    connection: Option<Box<ServerConnection<P>>>,
}

impl<P: Protocol> fmt::Debug for ServerSlot<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerSlot")
            .field("connected", &self.connection.is_some())
            .finish_non_exhaustive()
    }
}

impl<P: Protocol> ServerSlot<P> {
    pub fn new(server: Box<dyn Server<P>>) -> Self {
        Self {
            server,
            connection: None,
        }
    }

    pub fn reader(&mut self) -> Result<&mut dyn Reader<P, Downstream>, Error> {
        let Some(connection) = self.connection.as_mut() else {
            let error = Error::new("Server is not connected");
            observation::failed::<Downstream>("read", &error);
            return Err(error);
        };
        Ok(connection.reader())
    }

    /// 最终关闭失败只记录诊断，不覆盖已经完成或已经失败的业务结果。
    pub async fn close(&mut self) {
        if let Some(mut connection) = self.connection.take()
            && let Err(error) = connection.shutdown().await
        {
            tracing::warn!(
                target: "intercept_proxy::exchange::diagnostic",
                endpoint = "server",
                error = %error,
                "Server connection shutdown failed"
            );
        }
    }
}

#[async_trait]
impl<P: ObservedProtocol> Writer<P, Upstream> for ServerSlot<P> {
    async fn write(&mut self, context: P::Context) -> Result<P::Context, Error> {
        if self.connection.is_none() {
            let connection = self.server.connect(&context).await.inspect_err(|error| {
                observation::failed_with_context::<P, Upstream>("connect", &context, error);
            })?;
            self.connection = Some(connection);
        }

        let connection = self.connection.as_mut().ok_or_else(|| {
            let error = Error::new("Server connection was not retained");
            observation::failed::<Upstream>("connect", &error);
            error
        })?;
        connection.writer().write(context).await
    }
}
