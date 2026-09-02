//! Socket Pipeline 的资源边界装饰器。
//!
//! 真正的 Frame/Decode/Display/Rules/Encode 由 capability factory 创建。本模块只执行
//! Listener/Settings 已校验的 buffer/output 上限，不替代或合并任何协议阶段。

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Direction, Document, Encode, Error, Frame, FrameResult, Socket, SocketContext,
};

use super::super::SocketProcessingFailureKind;

pub(super) struct BoundedFrame<D: Direction> {
    inner: Box<dyn Frame<D>>,
    max_buffer_bytes: usize,
}

impl<D: Direction> BoundedFrame<D> {
    pub(super) fn new(inner: Box<dyn Frame<D>>, max_buffer_bytes: usize) -> Self {
        Self {
            inner,
            max_buffer_bytes,
        }
    }
}

#[async_trait]
impl<D: Direction> Frame<D> for BoundedFrame<D> {
    async fn split(&mut self, buffer: &[u8]) -> Result<FrameResult, Error> {
        if buffer.len() > self.max_buffer_bytes {
            return Err(stage_error::<D>(
                SocketProcessingFailureKind::BufferLimitExceeded,
                "frame exceeds the configured buffer limit",
            ));
        }
        self.inner.split(buffer).await
    }
}

pub(super) struct BoundedEncode<D: Direction> {
    inner: Box<dyn Encode<Socket, D>>,
    max_output_bytes: usize,
}

impl<D: Direction> BoundedEncode<D> {
    pub(super) fn new(inner: Box<dyn Encode<Socket, D>>, max_output_bytes: usize) -> Self {
        Self {
            inner,
            max_output_bytes,
        }
    }
}

#[async_trait]
impl<D: Direction> Encode<Socket, D> for BoundedEncode<D> {
    async fn encode(
        &mut self,
        original: &SocketContext,
        document: &Document,
    ) -> Result<SocketContext, Error> {
        let context = self.inner.encode(original, document).await?;
        if context.data.is_empty() {
            return Err(stage_error::<D>(
                SocketProcessingFailureKind::EmptyOutput,
                "Encode returned an empty Socket context",
            ));
        }
        if context.data.len() > self.max_output_bytes {
            return Err(stage_error::<D>(
                SocketProcessingFailureKind::OutputLimitExceeded,
                "Encode output exceeds the configured limit",
            ));
        }
        Ok(context)
    }
}

pub(super) fn factory_panicked() -> Error {
    Error::new(format!(
        "{}: socket capability factory panicked",
        SocketProcessingFailureKind::ProcessorPanicked.as_str()
    ))
}

pub(super) fn factory_failed(error: &super::super::SocketProcessingFailure) -> Error {
    Error::new(format!(
        "{}: capability factory failed",
        error.stable_code()
    ))
}

fn stage_error<D: Direction>(
    kind: SocketProcessingFailureKind,
    message: impl Into<String>,
) -> Error {
    Error::new(format!(
        "{:?}|{}: {}",
        D::KIND,
        kind.as_str(),
        message.into()
    ))
}
