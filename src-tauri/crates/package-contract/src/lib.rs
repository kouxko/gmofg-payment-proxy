//! Final API 1 package wire contract.
//!
//! This crate is the single owner of package Manifest, fixed JSON-RPC envelopes and method
//! parameters, frame results, and stable-code RPC error data. It reuses the protocol-neutral
//! [`intercept_proxy_domain`] Document, Schema, identity, and error-code types.
#![deny(missing_docs)]

mod frame;
mod manifest;
mod rpc;

pub use frame::*;
pub use manifest::*;
pub use rpc::*;
