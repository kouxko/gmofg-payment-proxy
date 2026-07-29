//! Application use cases and IPC-facing view models.
//!
//! This crate owns the ports used by infrastructure and runtime adapters. It deliberately
//! contains no Tauri, database, TLS, or filesystem implementation.

mod breakpoint_validation;
mod breakpoints;
mod error;
mod events;
mod facade;
mod models;
mod ports;
mod sessions;

pub use breakpoint_validation::BreakpointValidator;
pub use breakpoints::{BreakpointCoordinator, BreakpointOutcome, BreakpointTicket};
pub use error::{AppError, AppErrorViewModel, AppResult};
pub use events::{EventHub, EventReplay, EventSubscription};
pub use facade::Application;
pub use models::*;
pub use ports::*;
pub use sessions::{InMemorySessionStore, SessionStore};

#[cfg(test)]
mod requirements_tests;
