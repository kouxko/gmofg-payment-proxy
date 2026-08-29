//! Embedded Model Context Protocol server.
//!
//! This outer adapter owns all-interface plaintext HTTP and JSON-RPC. Reads and
//! writes cross only typed [`intercept_proxy_application::Application`]
//! boundaries; MCP does not own database, file, runtime, or persistence logic.

mod backend;
mod catalog;
pub mod environment_contract;
mod protocol;
mod query;
mod resources;
mod server;

pub use backend::ApplicationBackend;
#[cfg(test)]
pub(crate) use backend::McpBackend;
pub(crate) use server::McpIpCapability;
pub use server::McpServer;

/// Bind projection shown to users. Clients must replace `0.0.0.0` with this
/// machine's reachable address; it is not presented as a fabricated client URL.
pub const MCP_BIND_ENDPOINT: &str = "http://0.0.0.0:17653/mcp";

pub fn catalog_size() -> (usize, usize) {
    (catalog::tools().len(), resources::list().len())
}

#[cfg(test)]
mod tests;
