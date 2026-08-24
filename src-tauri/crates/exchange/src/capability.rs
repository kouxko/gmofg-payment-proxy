//! Frame、Decode、Display、Rules 与 Encode 的可替换能力端口。

use async_trait::async_trait;
use intercept_proxy_domain::Document;

use crate::{Direction, Error, Protocol};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameResult {
    /// 当前缓冲区不足；Reader 继续执行普通 read，不猜测还需要多少字节。
    NeedMore,
    /// `consumed` 是当前缓冲区内唯一完整 Frame 的长度。
    Complete { consumed: usize },
}

#[async_trait]
pub trait Frame<D: Direction>: Send {
    async fn split(&mut self, buffer: &[u8]) -> Result<FrameResult, Error>;
}

#[async_trait]
pub trait Decode<P: Protocol, D: Direction>: Send {
    async fn decode(&mut self, context: &P::Context) -> Result<Document, Error>;
}

/// 项目自定义的展示能力；与 `std::fmt::Display` 无关。
#[async_trait]
pub trait Display: Send {
    async fn display(&mut self, document: &Document) -> Result<String, Error>;
}

/// Rules 接收 Reader 事实 Document 的 clone，并返回独立的写出 Document。
#[async_trait]
pub trait Rules: Send {
    async fn apply(&mut self, document: Document) -> Result<Document, Error>;
}

#[async_trait]
pub trait Encode<P: Protocol, D: Direction>: Send {
    async fn encode(
        &mut self,
        original: &P::Context,
        document: &Document,
    ) -> Result<P::Context, Error>;
}
