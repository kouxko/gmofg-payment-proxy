//! Embedded read-only Model Context Protocol server.
//!
//! This outer adapter owns only loopback HTTP and JSON-RPC. Every application
//! fact is read through [`intercept_proxy_application::Application`]; no MCP
//! component owns a database connection, file writer, network control or
//! mutation capability.

mod backend;
mod catalog;
mod protocol;
mod query;
mod resources;
mod server;

pub use backend::ApplicationBackend;
pub use server::{MCP_ENDPOINT, ReadOnlyMcpServer};

#[cfg(test)]
mod tests;
