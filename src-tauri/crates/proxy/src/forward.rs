//! 标准 HTTP/1.1 正向代理与 CONNECT 隧道。
//!
//! 具体协议实现按监听生命周期、CONNECT、HTTP、WebSocket 与 MITM 会话拆分在
//! `service` 子模块；本文件只保留稳定的公开入口。

mod service;

pub use service::*;
pub(crate) use service::{authority_is_allowed, config_error};
