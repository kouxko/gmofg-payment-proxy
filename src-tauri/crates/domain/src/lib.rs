//! 可复用拦截代理的纯领域类型和业务规则。
//!
//! 本 crate 刻意不依赖 Tauri、存储、TLS 或异步运行时，是领域真相及 IPC 数据形状的
//! 稳定来源。
#![allow(clippy::missing_errors_doc)]

pub mod android_network;
pub mod breakpoint;
pub mod certificate;
pub mod document;
pub mod error;
pub mod external_package;
pub mod id;
pub mod json_path;
pub mod message;
pub mod protocol_document_rule;
pub mod protocol_package;
pub mod revision;
pub mod rule;
pub mod session;
pub mod settings;
pub mod state;
pub mod unified_rule;
pub mod unified_rule_execution;
pub mod workspace;

#[cfg(test)]
mod android_network_tests;

pub use android_network::*;
pub use breakpoint::*;
pub use certificate::*;
pub use document::*;
pub use error::*;
pub use external_package::*;
pub use id::*;
pub use json_path::*;
pub use message::*;
pub use protocol_document_rule::*;
pub use protocol_package::*;
pub use revision::*;
pub use rule::*;
pub use session::*;
pub use settings::*;
pub use state::*;
pub use unified_rule::*;
pub use unified_rule_execution::*;
pub use workspace::*;
