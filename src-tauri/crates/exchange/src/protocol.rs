//! 协议上下文与数据方向。
//!
//! Reader 只返回协议自己的 `Context`：HTTP 是 header/body 文本，Socket 是原始字节。
//! 方向由类型参数固定，防止 upstream/downstream 的 Reader、Writer 被接反。

use std::fmt::Debug;

pub trait Protocol: Send + Sync + 'static {
    type Context: Clone + Debug + Send + Sync + 'static;
}

/// tracing 观测使用的无损 Context 借用视图。
///
/// 它不改变协议 Context，也不提供通用 `evidence()` 类型抹平。HTTP 仍保存 header/body
/// 文本，Socket 仍保存原始字节；该视图只负责把强类型值投影为 tracing 的 primitive 字段。
#[derive(Debug)]
#[doc(hidden)]
pub enum ObservedContext<'a> {
    Http {
        header: &'a str,
        body: &'a str,
        body_is_utf8: bool,
    },
    Socket {
        data: &'a [u8],
    },
}

#[doc(hidden)]
pub trait ObservedProtocol: Protocol {
    const NAME: &'static str;

    fn observed(context: &Self::Context) -> ObservedContext<'_>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Http;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Socket;

/// HTTP 已经是文本协议，因此保留未经类型抹平的 header/body 文本。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HttpContext {
    pub header: String,
    pub body: String,
    /// `false` means `body` is only a lossy display projection of the wire bytes.
    pub body_is_utf8: bool,
    /// Authoritative framed HTTP Body bytes. Text is only a decoded/display projection.
    pub wire_body: Vec<u8>,
}

/// Socket 协议模式的一次 transport read 或一个完整 Frame 所持有的字节。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SocketContext {
    pub data: Vec<u8>,
}

impl Protocol for Http {
    type Context = HttpContext;
}

impl Protocol for Socket {
    type Context = SocketContext;
}

impl ObservedProtocol for Http {
    const NAME: &'static str = "http";

    fn observed(context: &Self::Context) -> ObservedContext<'_> {
        ObservedContext::Http {
            header: &context.header,
            body: &context.body,
            body_is_utf8: context.body_is_utf8,
        }
    }
}

impl ObservedProtocol for Socket {
    const NAME: &'static str = "socket";

    fn observed(context: &Self::Context) -> ObservedContext<'_> {
        ObservedContext::Socket {
            data: &context.data,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectionKind {
    Upstream,
    Downstream,
}

pub trait Direction: Send + Sync + 'static {
    const KIND: DirectionKind;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Upstream;

impl Direction for Upstream {
    const KIND: DirectionKind = DirectionKind::Upstream;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Downstream;

impl Direction for Downstream {
    const KIND: DirectionKind = DirectionKind::Downstream;
}
