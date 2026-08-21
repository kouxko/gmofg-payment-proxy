//! 外部 Socket 协议包的纯领域合同。
//!
//! 本模块只定义第三方进程注册和处理报文时使用的严格数据形状。它复用 [`crate::DocumentSchema`]
//! 与 [`crate::Document`] 作为规则引擎的唯一领域模型，不包含 WebSocket、JSON-RPC 调度、持久化、
//! 并发控制或 UI 状态。
//!
//! 所有公开 wire 类型都拒绝未知字段；外部 Document 则在绑定方向 Schema 时拒绝未知字段名、
//! 类型错配、越界整数及非规范 Base64，避免不可信第三方数据绕过领域不变量。
#![deny(missing_docs)]

mod document;
mod registration;
mod runtime;

pub use document::*;
pub use registration::*;
pub use runtime::*;

#[cfg(test)]
mod tests;
