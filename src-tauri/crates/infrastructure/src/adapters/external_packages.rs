//! 外部软件包 WebSocket/JSON-RPC 连接适配器。
#![deny(missing_docs)]

mod handshake;

pub use handshake::accept_packages_websocket;
