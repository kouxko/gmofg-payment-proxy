//! Socket 协议包的纯领域模型。
//!
//! 本模块回答三个与具体脚本引擎无关的问题：
//!
//! 1. 使用哪个不可变协议包版本（[`ProtocolPackageRef`]）；
//! 2. 协议包允许产生哪些稳定字段（[`DocumentSchema`]）；
//! 3. 当前完整 Frame 实际解码出了哪些字段值（[`Document`]）。
//!
//! # Schema 与协议校验的边界
//!
//! Schema 只声明字段名称、值类型和 UI 标签。它不声明 `required`：字段是否必须出现通常取决于
//! 消息类型、方向或其他字段，必须由具体协议脚本判断。未出现在当前报文中的已声明字段保留为空槽，
//! 可用 [`Document::has`] 区分；读取空槽时 [`Document::get`] 返回稳定错误。
//!
//! # 不变量与序列化
//!
//! 所有标识符和聚合字段都是私有的，只能通过受校验构造器创建。反序列化同样回到这些构造器，
//! 因而 JSON/IPC 不能绕过 ID、Schema 或值类型约束。字段和值槽始终采用 Schema 声明顺序。
//!
//! # 边界
//!
//! 本模块不包含脚本源码、Rhai AST、ZIP、数据库、Socket 或发送函数。包读取、脚本编译和网络执行
//! 属于外层 crate，避免 Direct Socket relay 被脚本依赖污染。
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
//! assert!(!document.has("amount")?);
//! document.set("amount", DocumentValue::Int(1000))?;
//! assert_eq!(document.get("amount")?, &DocumentValue::Int(1000));
//! # Ok::<(), intercept_proxy_domain::DomainError>(())
//! ```
// 新增协议包公开 API 时必须同时解释其领域语义，避免只增加可调用接口而没有作者契约。
#![deny(missing_docs)]

mod document;
mod identity;
mod schema;

pub use document::*;
pub use identity::*;
pub use schema::*;

#[cfg(test)]
mod tests;
