//! 外部软件包 WebSocket/JSON-RPC 连接适配器。
#![deny(missing_docs)]

mod actor;
mod error;
mod handshake;

pub use actor::{ExternalPackageClient, ExternalPackageConnectionConfig};
#[cfg(test)]
pub use error::ExternalPackageRemoteError;
pub use error::{ExternalPackageConnectionError, ExternalPackageFatalProtocolError};
pub use handshake::accept_packages_websocket;

#[cfg(test)]
mod tests;
