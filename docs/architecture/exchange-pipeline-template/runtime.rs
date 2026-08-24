//! Exchange 运行时设计模板。本文件不参与当前工程编译。

use async_trait::async_trait;
use tracing::Instrument;

use super::core::{
    Direction, DirectionKind, Downstream, Error, Pipeline, Protocol, Reader, Socket, Upstream,
    Writer,
};
use super::transparent::TransparentExchange;

/// Exchange 内部关联标识：只用于 tracing/UI 事件归属，不进入协议数据。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExchangeId(u64);

impl ExchangeId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }
}

/// 一条全双工连接拥有 Reader、Writer 和自己的生命周期。
/// RD/WD 表示从该连接读取、向该连接写入时对应的业务方向。
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

/// App 连接：读取 Upstream，写入 Downstream。
pub type AppConnection<P> = dyn Connection<P, Upstream, Downstream>;

/// Server 连接：读取 Downstream，写入 Upstream。
pub type ServerConnection<P> = dyn Connection<P, Downstream, Upstream>;

#[async_trait]
pub trait Server<P: Protocol>: Send {
    async fn connect(&mut self, context: &P::Context) -> Result<Box<ServerConnection<P>>, Error>;
}

/// RemoteServer 和 LocalServer 都放在这个 slot 中。
/// 一个 App Connection 在创建 Exchange 时即固定一个 Server Endpoint；
/// 第一次 upstream Writer.write() 才建立该 Endpoint 的实际连接，之后不得切换目标。
pub struct ServerSlot<P: Protocol> {
    server: Box<dyn Server<P>>,
    connection: Option<Box<ServerConnection<P>>>,
}

impl<P: Protocol> ServerSlot<P> {
    pub fn new(server: Box<dyn Server<P>>) -> Self {
        Self {
            server,
            connection: None,
        }
    }

    pub fn reader(&mut self) -> Result<&mut dyn Reader<P, Downstream>, Error> {
        self.connection
            .as_mut()
            .map(|connection| connection.reader())
            .ok_or_else(|| {
                let error = Error::new("Server is not connected");
                tracing::error!(
                    target: "intercept_proxy::exchange::ui",
                    event = "failed",
                    direction = ?Downstream::KIND,
                    stage = "read",
                    error = %error.message
                );
                error
            })
    }

    pub async fn close(&mut self) {
        if let Some(mut connection) = self.connection.take() {
            if let Err(error) = connection.shutdown().await {
                tracing::warn!(
                    target: "intercept_proxy::exchange::diagnostic",
                    endpoint = "server",
                    error = %error.message,
                    "Server connection shutdown failed"
                );
            }
        }
    }
}

#[async_trait]
impl<P: Protocol> Writer<P, Upstream> for ServerSlot<P> {
    async fn write(&mut self, context: P::Context) -> Result<(), Error> {
        if self.connection.is_none() {
            self.connection = Some(self.server.connect(&context).await.map_err(|error| {
                tracing::error!(
                    target: "intercept_proxy::exchange::ui",
                    event = "failed",
                    direction = ?Upstream::KIND,
                    stage = "connect",
                    context = ?context,
                    error = %error.message
                );
                error
            })?);
        }
        self.connection
            .as_mut()
            .expect("ServerSlot connected above")
            .writer()
            .write(context)
            .await
    }
}

/// 协议模式运行器：直接持有 upstream/downstream 两个 Pipeline，不定义 Flow。
pub struct ProtocolExchange<P>
where
    P: Protocol,
{
    app: Box<AppConnection<P>>,
    server: ServerSlot<P>,
    upstream: Pipeline<P, Upstream>,
    downstream: Pipeline<P, Downstream>,
}

impl<P> ProtocolExchange<P>
where
    P: Protocol,
{
    pub fn new(
        app: Box<AppConnection<P>>,
        server: ServerSlot<P>,
        upstream: Pipeline<P, Upstream>,
        downstream: Pipeline<P, Downstream>,
    ) -> Self {
        Self {
            app,
            server,
            upstream,
            downstream,
        }
    }

    pub async fn exchange(mut self) -> Result<(), Error> {
        let result = self.run().await;
        if let Err(error) = self.app.shutdown().await {
            tracing::warn!(
                target: "intercept_proxy::exchange::diagnostic",
                endpoint = "app",
                error = %error.message,
                "App connection shutdown failed"
            );
        }
        self.server.close().await;
        result
    }

    async fn run(&mut self) -> Result<(), Error> {
        loop {
            let Some(request) = self.upstream.read(self.app.reader()).await? else {
                // App EOF：Exchange 正常结束。
                return Ok(());
            };
            self.record_received(&request);

            let sent = match self.upstream.write(&mut self.server, &request).await {
                Ok(sent) => sent,
                Err(error) => return Err(error),
            };
            self.record_sent::<Upstream>(&sent);

            // 一次协议交换严格配对。写完 Server 后只读取 Server 回复，
            // 不并发 poll App Reader；App 提前写入的数据留在 transport 缓冲区，
            // 完成 downstream 写入后才在下一轮读取。
            let response = match self.downstream.read(self.server.reader()?).await? {
                Some(response) => response,
                None => {
                    let error = Error::new("Server disconnected before replying");
                    return self.fail(Downstream::KIND, "read", error);
                }
            };
            self.record_received(&response);

            let sent = match self.downstream.write(self.app.writer(), &response).await {
                Ok(sent) => sent,
                Err(error) => return Err(error),
            };
            self.record_sent::<Downstream>(&sent);
        }
    }

    fn record_received<D>(&self, envelope: &super::core::Envelope<P, D>)
    where
        D: Direction,
    {
        tracing::info!(
            target: "intercept_proxy::exchange::ui",
            event = "received",
            direction = ?D::KIND,
            context = ?envelope.context(),
            document = ?envelope.document(),
            display = %envelope.display()
        );
    }

    fn record_sent<D: Direction>(&self, context: &P::Context) {
        tracing::info!(
            target: "intercept_proxy::exchange::ui",
            event = "sent",
            direction = ?D::KIND,
            context = ?context
        );
    }

    fn fail(
        &self,
        direction: DirectionKind,
        stage: &'static str,
        error: Error,
    ) -> Result<(), Error> {
        tracing::error!(
            target: "intercept_proxy::exchange::ui",
            event = "failed",
            direction = ?direction,
            stage = stage,
            error = %error.message
        );
        Err(error)
    }
}

/// 一个 accepted App connection 只创建一个 Exchange。
/// 外层只负责连接级生命周期；具体数据通路由内部模式负责。
pub struct Exchange<P>
where
    P: Protocol,
{
    id: ExchangeId,
    mode: ExchangeMode<P>,
}

enum ExchangeMode<P>
where
    P: Protocol,
{
    Protocol(ProtocolExchange<P>),
    Transparent(TransparentExchange),
}

impl<P> Exchange<P>
where
    P: Protocol,
{
    pub fn protocol(id: ExchangeId, exchange: ProtocolExchange<P>) -> Self {
        Self {
            id,
            mode: ExchangeMode::Protocol(exchange),
        }
    }

    /// 外部 accept loop 只需要 poll 这一个方法。
    pub async fn exchange(self) -> Result<(), Error> {
        let span = tracing::info_span!(
            target: "intercept_proxy::exchange",
            "exchange",
            exchange_id = self.id.0
        );
        async move {
            tracing::info!(
                target: "intercept_proxy::exchange::ui",
                event = "opened"
            );

            let result = match self.mode {
                ExchangeMode::Protocol(exchange) => exchange.exchange().await,
                ExchangeMode::Transparent(exchange) => exchange.exchange().await,
            };

            match &result {
                Ok(()) => tracing::info!(
                    target: "intercept_proxy::exchange::ui",
                    event = "closed",
                    outcome = "completed"
                ),
                Err(error) => tracing::error!(
                    target: "intercept_proxy::exchange::ui",
                    event = "closed",
                    outcome = "failed",
                    error = %error.message
                ),
            }
            result
        }
        .instrument(span)
        .await
    }
}

impl Exchange<Socket> {
    /// 只有 Socket Exchange 暴露透明模式构造入口。
    pub fn transparent(id: ExchangeId, exchange: TransparentExchange) -> Self {
        Self {
            id,
            mode: ExchangeMode::Transparent(exchange),
        }
    }
}

// 协议模式：RemoteServer<P> 和 LocalServer<P> 都实现 Server<P>。
// 透明模式：RemoteRawServer 和 LocalRawServer 都实现 RawServer。
// LocalServer 建立进程内 Echo channel，RemoteServer 建立真实 TCP/TLS channel。
