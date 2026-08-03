//! 应用用例与前端无关的展示模型。
//!
//! 规范化值、中文状态、色调语义、操作权限、分页、校验和稳定错误都由 Rust 负责，
//! Tauri、未来 TUI/CLI 只渲染同一份协议，不重复作业务决定。这些模型不绑定具体组件库。
//!
//! 本 crate 还定义由 infrastructure 适配器实现的端口，但不包含 Tauri、数据库、TLS
//! 或文件系统实现。

mod android;
mod breakpoint_validation;
mod breakpoints;
mod capacity;
mod configuration;
mod error;
mod events;
mod facade;
mod listeners;
mod models;
mod ports;
mod sessions;
mod workspaces;

pub use android::*;
pub use breakpoint_validation::BreakpointValidator;
pub use breakpoints::{BreakpointCoordinator, BreakpointOutcome, BreakpointTicket};
pub use capacity::CapacityLedger;
pub use configuration::*;
pub use error::{AppError, AppErrorViewModel, AppResult};
pub use events::{EventHub, EventReplay, EventSubscription};
pub use facade::{Application, ApplicationDependencies};
pub use listeners::InMemoryListenerRuntime;
pub use models::*;
pub use ports::*;
pub use sessions::{InMemorySessionStore, SessionStore};
pub use workspaces::{
    InMemoryWorkspaceDocumentStore, InMemoryWorkspaceStore, remap_workspace_identity,
};

#[cfg(test)]
mod requirements_tests;
