//! 应用层输入模型、ViewModel 与实时事件 DTO。
//!
//! 输入模型表达“用户想做什么”，ViewModel 表达“界面应显示什么”。各领域模型按页面和
//! 运行时职责拆分；本模块统一重导出，保持原有 `models::*` 公共 API 稳定。

mod breakpoint;
mod capture;
mod certificate_settings;
mod common;
mod diagnostics;
mod events;
mod fault;
mod protocol_package;
mod protocol_rule;
mod rule;
mod session;
mod socket_capture;
mod socket_capture_diagnostics;
mod workspace;

#[cfg(test)]
mod socket_capture_tests;

pub use breakpoint::*;
pub use capture::*;
pub use certificate_settings::*;
pub use common::*;
pub use diagnostics::*;
pub use events::*;
pub use fault::*;
pub use protocol_package::*;
pub use protocol_rule::*;
pub use rule::*;
pub use session::*;
pub use socket_capture::*;
pub use socket_capture_diagnostics::*;
pub use workspace::*;
