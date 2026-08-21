//! 仅允许 TLS 1.2 的 rustls 双向认证适配器（`PROXY-001` 至 `PROXY-005`）。
//!
//! 下游服务端要求受信客户端证书，上游客户端同时验证服务器链/主机名并提交客户端身份。
//! 握手受超时和取消令牌控制；证书、协议版本或主机名不匹配都在进入 HTTP pipeline 前
//! 封闭失败，调试输出不得包含私钥字节。

mod client;
mod server;
mod support;

pub use client::{ClientTlsAdapter, ClientTlsConnection, ClientTlsHandshakeEvidence};
pub use server::ServerTlsAdapter;
pub(crate) use support::peer_identity;
