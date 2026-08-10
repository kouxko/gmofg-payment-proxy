//! 有序实时事件、回放与订阅生命周期。
//!
//! `EventHub` 连接事件生产者与 Tauri、未来 TUI 或测试适配器。实现按回放、实时接收、
//! 抓包合批和容量管理拆分，避免单一文件同时承担所有事件职责。

mod capture;
mod diagnostics;
mod hub;
mod receiver;
mod replay;
mod retention;
mod storage;
mod subscription;
mod types;

pub(crate) use diagnostics::stage_for_error_code;
pub use receiver::TrackedEventReceiver;
pub use replay::{EventReplay, TrackedReplay};
pub use types::{EventHub, EventSubscription};
