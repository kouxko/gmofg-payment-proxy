//! Frame 入口的只读 Reader、裁决结果与单方向有界 FIFO。
//!
//! 本模块只处理 TCP chunk -> 完整 Frame。它不解析 Document，也不负责 Encode 或 Display。
//! 外层连接为两个方向各创建一个切帧器，从类型所有权上隔离 FIFO 状态。

// 生产代理接线位于后续 T20/T21；T10 先完整实现并验证协议运行时边界。
#![allow(dead_code)]

mod buffer;
mod deadline;
mod decision;
mod error;
mod framer;
mod inspector;
mod limits;
mod reader;
mod script;

use buffer::FrameBuffer;
use deadline::FrameCallDeadline;
use decision::{FramingDecision, validate_decision};
use error::ProtocolFramingResult;
pub use error::{ProtocolFramingError, ProtocolFramingErrorCode, ProtocolFramingLimit};
use framer::FrameDecider;
#[cfg(test)]
pub(crate) use framer::SingleDirectionFramer;
pub use inspector::{ProtocolFrameInspection, ProtocolFrameInspector};
pub use limits::{
    DEFAULT_MAX_FRAME_BYTES, DEFAULT_MAX_FRAME_FIFO_BYTES, MAX_FRAME_BYTES_LIMIT,
    MAX_FRAME_FIFO_BYTES_LIMIT, ProtocolFramingLimits,
};
use reader::{ProtocolReader, ReaderSegment};
pub(crate) use script::RhaiFrameDecider;

pub(crate) fn register(engine: &mut rhai::Engine) {
    reader::register(engine);
    decision::register(engine);
}

#[cfg(test)]
mod tests;
