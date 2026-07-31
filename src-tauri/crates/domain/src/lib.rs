//! Pure domain types and business rules for the reusable interception proxy.
//!
//! This crate deliberately has no dependency on Tauri, storage, TLS, or async
//! runtimes. It is the stable source for domain and IPC-facing data shapes.
#![allow(clippy::missing_errors_doc)]

pub mod breakpoint;
pub mod certificate;
pub mod error;
pub mod id;
pub mod json_path;
pub mod message;
pub mod revision;
pub mod rule;
pub mod session;
pub mod settings;
pub mod state;

pub use breakpoint::*;
pub use certificate::*;
pub use error::*;
pub use id::*;
pub use json_path::*;
pub use message::*;
pub use revision::*;
pub use rule::*;
pub use session::*;
pub use settings::*;
pub use state::*;
