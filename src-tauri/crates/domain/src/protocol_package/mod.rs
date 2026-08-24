//! 协议包不可变身份的纯领域模型。
//!
//! 协议包只负责稳定 ID 与版本引用。协议无关的结构化数据模型位于
//! [`crate::document`]，脚本源码、Schema 文件解析、ZIP 和运行时执行属于外层 crate。
#![deny(missing_docs)]

mod identity;

pub use identity::*;

#[cfg(test)]
mod tests;
