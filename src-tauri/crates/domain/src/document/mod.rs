//! 协议无关的结构化 Document 领域模型。
//!
//! 本模块定义 [`Document`]、[`DocumentSchema`] 和类型化字段值。它只维护 Schema 身份、
//! 字段顺序、字段 presence 与类型一致性，不感知 HTTP、Socket、Frame、脚本引擎、数据库或
//! 网络连接。
//!
//! TOML、Rhai 和外部进程协议属于边界适配器：它们必须先把不可信输入转换并校验为本模块的
//! 类型，之后 Exchange、Rules 和 Encode 才共享同一份领域契约。
//!
//! # 示例
//!
//! ```
//! use intercept_proxy_domain::{
//!     Document, DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema,
//!     DocumentSchemaId, DocumentValue,
//! };
//!
//! let schema = DocumentSchema::new(
//!     DocumentSchemaId::new("payment-message")?,
//!     1,
//!     "Payment Message",
//!     vec![DocumentField::new(
//!         DocumentFieldName::new("amount")?,
//!         DocumentFieldType::Int,
//!         "Amount",
//!     )?],
//! )?;
//! let mut document = Document::new(schema);
//! document.set("amount", DocumentValue::Int(1000))?;
//! assert_eq!(document.get("amount")?, &DocumentValue::Int(1000));
//! # Ok::<(), intercept_proxy_domain::DomainError>(())
//! ```
#![deny(missing_docs)]

mod model;
mod schema;

pub use model::*;
pub use schema::*;

#[cfg(test)]
mod tests;
