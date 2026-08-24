//! 连接级 Exchange runner 与严格配对的协议数据流。

use std::{fmt, future::Future, panic::AssertUnwindSafe};

use tracing::Instrument;

use crate::{
    AppConnection, Direction, DirectionKind, Downstream, Envelope, Error, Pipeline, Protocol,
    ServerSlot, Socket, Upstream,
};
use crate::{ObservedProtocol, observation};

use crate::transparent::TransparentExchange;

/// 仅用于并发 tracing/UI 事件归属，不进入协议、Envelope 或普通业务展示。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExchangeId(u128);

impl ExchangeId {
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u128 {
        self.0
    }

    pub fn trace_value(self) -> String {
        format!("{:032x}", self.0)
    }
}

/// 协议模式直接持有两个方向的 Pipeline，不增加 Flow 或 Reader/Writer role 泛型。
pub struct ProtocolExchange<P: Protocol> {
    app: Box<AppConnection<P>>,
    server: ServerSlot<P>,
    upstream: Pipeline<P, Upstream>,
    downstream: Pipeline<P, Downstream>,
}

impl<P: Protocol> fmt::Debug for ProtocolExchange<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProtocolExchange")
            .field("protocol", &std::any::type_name::<P>())
            .finish_non_exhaustive()
    }
}

impl<P: ObservedProtocol> ProtocolExchange<P> {
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

        // 完整连接关闭发生在业务循环结束之后。关闭失败只追加诊断，绝不把已成功
        // 的交易改成失败，也不覆盖 `result` 中已经存在的业务错误。
        if let Err(error) = self.app.shutdown().await {
            tracing::warn!(
                target: "intercept_proxy::exchange::diagnostic",
                endpoint = "app",
                error = %error,
                "App connection shutdown failed"
            );
        }
        self.server.close().await;
        result
    }

    async fn run(&mut self) -> Result<(), Error> {
        loop {
            // 严格顺序：只 poll 当前步骤。等待 Server 回复期间不读取或探测 App，
            // 提前到达的下一笔数据由 transport 缓冲到下一轮。
            let Some(request) = self.upstream.read(self.app.reader()).await? else {
                return Ok(());
            };
            record_received(&request);

            let sent = self.upstream.write(&mut self.server, &request).await?;
            record_sent::<P, Upstream>(&sent);

            let Some(response) = self.downstream.read(self.server.reader()?).await? else {
                return fail(
                    Downstream::KIND,
                    "read",
                    "Server disconnected before replying",
                );
            };
            record_received(&response);

            let sent = self.downstream.write(self.app.writer(), &response).await?;
            record_sent::<P, Downstream>(&sent);
        }
    }
}

fn record_received<P: ObservedProtocol, D: Direction>(envelope: &Envelope<P, D>) {
    observation::received(envelope);
}

fn record_sent<P: ObservedProtocol, D: Direction>(context: &P::Context) {
    observation::sent::<P, D>(context);
}

fn fail<T>(
    direction: DirectionKind,
    stage: &'static str,
    message: &'static str,
) -> Result<T, Error> {
    let error = Error::new(message);
    let direction = match direction {
        DirectionKind::Upstream => "upstream",
        DirectionKind::Downstream => "downstream",
    };
    tracing::error!(
        target: "intercept_proxy::exchange::ui",
        event = "failed",
        direction,
        stage,
        error = error.message.as_str(),
    );
    Err(error)
}

/// 一个 accepted App connection 只创建一个外层 Exchange。
pub struct Exchange<P: Protocol> {
    id: ExchangeId,
    mode: ExchangeMode<P>,
}

impl<P: Protocol> fmt::Debug for Exchange<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Exchange")
            .field("id", &self.id)
            .field("protocol", &std::any::type_name::<P>())
            .finish_non_exhaustive()
    }
}

enum ExchangeMode<P: Protocol> {
    Protocol(ProtocolExchange<P>),
    Transparent(TransparentExchange),
}

impl<P: ObservedProtocol> Exchange<P> {
    pub fn protocol(id: ExchangeId, exchange: ProtocolExchange<P>) -> Self {
        Self {
            id,
            mode: ExchangeMode::Protocol(exchange),
        }
    }

    /// Opens the connection-level Exchange before constructing protocol capabilities.
    ///
    /// A factory error or panic therefore remains observable as the same connection's
    /// `opened -> failed(capability_factory) -> closed` timeline. No fallback Pipeline is built.
    pub async fn protocol_with<F>(id: ExchangeId, build: F) -> Result<(), Error>
    where
        F: FnOnce() -> Result<ProtocolExchange<P>, Error>,
    {
        Self::run_observed(id, async move {
            let protocol = match std::panic::catch_unwind(AssertUnwindSafe(build)) {
                Ok(Ok(protocol)) => protocol,
                Ok(Err(error)) => {
                    capability_factory_failed(&error);
                    return Err(error);
                }
                Err(_) => {
                    let error =
                        Error::new("CAPABILITY_FACTORY_PANICKED: capability factory panicked");
                    capability_factory_failed(&error);
                    return Err(error);
                }
            };
            protocol.exchange().await
        })
        .await
    }

    /// accept loop 唯一需要 poll 的连接级入口。
    pub async fn exchange(self) -> Result<(), Error> {
        Self::run_observed(self.id, async move {
            match self.mode {
                ExchangeMode::Protocol(exchange) => exchange.exchange().await,
                ExchangeMode::Transparent(exchange) => exchange.exchange().await,
            }
        })
        .await
    }

    async fn run_observed(
        id: ExchangeId,
        operation: impl Future<Output = Result<(), Error>>,
    ) -> Result<(), Error> {
        let exchange_id = id.trace_value();
        let span = tracing::info_span!(
            target: "intercept_proxy::exchange",
            "exchange",
            exchange_id = exchange_id.as_str(),
            protocol = P::NAME,
        );
        async move {
            tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
            let result = operation.await;
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
                    error = error.message.as_str(),
                ),
            }
            result
        }
        .instrument(span)
        .await
    }
}

fn capability_factory_failed(error: &Error) {
    tracing::error!(
        target: "intercept_proxy::exchange::ui",
        event = "failed",
        stage = "capability_factory",
        error = error.message.as_str(),
    );
}

impl Exchange<Socket> {
    /// 透明构造入口只存在于 Socket，HTTP CONNECT/Upgrade 不受支持。
    pub fn transparent(id: ExchangeId, exchange: TransparentExchange) -> Self {
        Self {
            id,
            mode: ExchangeMode::Transparent(exchange),
        }
    }
}
