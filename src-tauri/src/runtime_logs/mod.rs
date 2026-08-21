//! 持久化运行日志以及 `tracing` 到应用日志的进程级桥接。

mod model;
mod store;
mod tracing_bridge;

pub(crate) use model::{
    ApplicationLogEntry, ApplicationLogLevel, ApplicationLogPage, ApplicationLogQuery,
};
pub(crate) use store::RuntimeLogStore;
pub(crate) use tracing_bridge::install_tracing_bridge;

#[cfg(test)]
mod tests;
