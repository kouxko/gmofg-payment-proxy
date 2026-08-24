//! Exchange 数据面的单一错误类型。
//!
//! 错误阶段由失败发生处直接写入 structured tracing；错误值只负责沿 `Result` 传播，
//! 避免业务控制流与 UI/诊断模型耦合。

#[derive(Clone, Debug, Eq, thiserror::Error, PartialEq)]
#[error("{message}")]
pub struct Error {
    pub message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
