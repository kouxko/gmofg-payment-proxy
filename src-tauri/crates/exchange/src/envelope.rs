//! Reader Pipeline 产生的不可变协议消息信封。

use std::{fmt, marker::PhantomData};

use intercept_proxy_domain::Document;

use crate::{Direction, Protocol};

/// `context/document/display` 都是在 read 完成时固定的事实。
/// Writer 只能读取；Rules 修改 clone，不会重写 UI 已经看到的内容。
pub struct Envelope<P: Protocol, D: Direction> {
    pub(crate) context: P::Context,
    pub(crate) document: Document,
    pub(crate) display: String,
    pub(crate) direction: PhantomData<D>,
}

impl<P: Protocol, D: Direction> Envelope<P, D> {
    pub fn context(&self) -> &P::Context {
        &self.context
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn display(&self) -> &str {
        &self.display
    }
}

impl<P: Protocol, D: Direction> fmt::Debug for Envelope<P, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Envelope")
            .field("context", &self.context)
            .field("document", &self.document)
            .field("display", &self.display)
            .field("direction", &D::KIND)
            .finish()
    }
}
