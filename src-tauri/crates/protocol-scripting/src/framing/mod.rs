//! Frame 入口的只读 Reader、裁决结果与单方向有界缓冲区限制。
//!
//! 本模块只处理 TCP chunk -> 完整 Frame。它不解析 Document，也不负责 Encode 或 Display。
//! 外层 Exchange 为两个方向各持有一个 Pipeline，并独占累计缓冲区与消费顺序。

mod deadline;
mod decision;
mod error;
mod inspector;
mod limits;
mod reader;
mod script;

use deadline::FrameCallDeadline;
use decision::{FramingDecision, validate_decision};
use error::ProtocolFramingResult;
pub use error::{ProtocolFramingError, ProtocolFramingErrorCode, ProtocolFramingLimit};
pub use inspector::{ProtocolFrameInspection, ProtocolFrameInspector};
pub use limits::ProtocolFramingLimits;
#[cfg(test)]
use limits::{
    DEFAULT_MAX_FRAME_BYTES, DEFAULT_MAX_FRAME_FIFO_BYTES, MAX_FRAME_BYTES_LIMIT,
    MAX_FRAME_FIFO_BYTES_LIMIT,
};
use reader::ProtocolReader;
pub(crate) use script::RhaiFrameDecider;

pub(crate) fn register(engine: &mut rhai::Engine) {
    reader::register(engine);
    decision::register(engine);
}

#[cfg(test)]
mod tests;
