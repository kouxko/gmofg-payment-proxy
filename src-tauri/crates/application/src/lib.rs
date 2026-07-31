//! Application use cases and frontend-neutral presentation models.
//!
//! Rust deliberately owns normalized values, Chinese status text, UI tone, permissions,
//! pagination, validation, and stable errors so Tauri, a future TUI, and a future CLI can
//! render one contract without reimplementing business decisions. These models are not
//! coupled to Tauri or a specific widget toolkit.
//!
//! This crate also owns the ports implemented by infrastructure adapters. It contains no
//! Tauri, database, TLS, or filesystem implementation.

mod breakpoint_validation;
mod breakpoints;
mod capacity;
mod error;
mod events;
mod facade;
mod models;
mod ports;
mod sessions;

pub use breakpoint_validation::BreakpointValidator;
pub use breakpoints::{BreakpointCoordinator, BreakpointOutcome, BreakpointTicket};
pub use capacity::CapacityLedger;
pub use error::{AppError, AppErrorViewModel, AppResult};
pub use events::{EventHub, EventReplay, EventSubscription};
pub use facade::{Application, ApplicationDependencies};
pub use models::*;
pub use ports::*;
pub use sessions::{InMemorySessionStore, SessionStore};

#[cfg(test)]
mod requirements_tests;
