//! 协议无关的连接级 Exchange 与强类型 Pipeline 核心。
//!
//! 本 crate 不依赖 TCP、TLS、Tauri、数据库或具体脚本 runtime。外层 transport 只实现
//! 这里定义的端口；核心以类型系统和单一顺序循环保证数据方向与交易配对。

// Document 是 Exchange capability contract 的组成部分。由核心 crate 统一重导出，外层
// transport runtime 不需要反向依赖 Domain crate。
pub use intercept_proxy_domain::{
    Document, DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema,
    DocumentSchemaId, DocumentValue, DomainError,
};

mod capability;
mod endpoint;
mod envelope;
mod error;
mod exchange;
mod local_server;
mod observation;
mod pipeline;
mod protocol;
mod transparent;

pub use capability::*;
pub use endpoint::*;
pub use envelope::*;
pub use error::*;
pub use exchange::*;
pub use local_server::*;
pub use pipeline::*;
pub use protocol::*;
pub use transparent::*;

#[cfg(test)]
mod tests;
