//! Socket Pipeline 的资源边界装饰器。
//!
//! 真正的 Frame/Decode/Display/Rules/Encode 由 capability factory 创建。本模块只执行
//! Listener/Settings 已校验的 buffer/output 上限，不替代或合并任何协议阶段。

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Direction, Envelope, Error, ErrorStage, Frame, FrameResult, PrepareWrite, Socket, SocketContext,
};

use super::super::{SocketProcessingFailureKind, validate_socket_output};

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
                ErrorStage::Frame,
                "frame exceeds the configured buffer limit",
            ));
        }
        self.inner.split(buffer).await
    }
}

pub(super) struct BoundedPrepare<D: Direction> {
    inner: Box<dyn PrepareWrite<Socket, D>>,
    max_output_bytes: usize,
}

impl<D: Direction> BoundedPrepare<D> {
    pub(super) fn new(inner: Box<dyn PrepareWrite<Socket, D>>, max_output_bytes: usize) -> Self {
        Self {
            inner,
            max_output_bytes,
        }
    }
}

#[async_trait]
impl<D: Direction> PrepareWrite<Socket, D> for BoundedPrepare<D> {
    async fn prepare(&mut self, envelope: &Envelope<Socket, D>) -> Result<SocketContext, Error> {
        let context = self.inner.prepare(envelope).await?;
        validate_socket_output::<D>(&context, self.max_output_bytes)?;
        Ok(context)
    }
}

pub(super) fn factory_panicked() -> Error {
    Error::coded(
        SocketProcessingFailureKind::ProcessorPanicked.as_str(),
        "socket capability factory panicked",
    )
    .with_stage(ErrorStage::CapabilityFactory)
}

pub(super) fn factory_failed(error: &super::super::SocketProcessingFailure) -> Error {
    Error::coded(error.stable_code(), "capability factory failed")
        .with_stage(ErrorStage::CapabilityFactory)
}

fn stage_error<D: Direction>(
    kind: SocketProcessingFailureKind,
    stage: ErrorStage,
    message: impl Into<String>,
) -> Error {
    Error::coded(kind.as_str(), message)
        .with_stage(stage)
        .with_direction(D::KIND)
}
