//! 协议无关的结构化 Document 领域模型。
//!
//! 本模块定义身份无关的递归 JSON [`Document`]、RFC 6901 [`JsonPointer`] 和
//! [`DocumentSchemaNode`] 元数据树，不感知协议、数据库或运行时。
//!
//! JSON、JavaScript 和外部进程协议属于边界适配器：它们必须先把不可信输入转换并校验为本模块的
//! 类型，之后 Exchange、Rules 和 Encode 才共享同一份领域契约。
//!
#![deny(missing_docs)]

mod model;
mod pointer;
mod schema;

pub use model::*;
pub use pointer::*;
pub use schema::*;

#[cfg(test)]
mod tests;
