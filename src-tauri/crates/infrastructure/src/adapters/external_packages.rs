//! 外部软件包 WebSocket/JSON-RPC 连接适配器。
#![deny(missing_docs)]

mod actor;
mod error;
mod handshake;

pub use actor::{ExternalPackageClient, ExternalPackageConnectionConfig};
pub use error::{
    ExternalPackageConnectionError, ExternalPackageFatalProtocolError, ExternalPackageRemoteError,
};
pub use handshake::accept_packages_websocket;

#[cfg(test)]
mod tests;
