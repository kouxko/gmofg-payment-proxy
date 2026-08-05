//! 事务式多监听器 Tokio supervisor（`STATE-001` 至 `STATE-009`）。
//!
//! 启动先绑定全部端口并构建全部服务，再创建 epoch 和任务，最后一次性发布 `Running`；
//! 中途失败通过资源析构回滚。运行时拥有根取消令牌、listener join handles 和 watchdog，
//! stop/restart 必须先通知 pipeline、取消任务并等待 join，避免端口或后台事件泄漏到下一代。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::SocketAddr;
use std::panic::AssertUnwindSafe;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::FutureExt;
use serde::{Deserialize, Deserializer, Serialize};
use tokio::sync::{Mutex, RwLock, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::message::MessageLimits;
use crate::transport::{
    BoundListener, ConnectionAdmission, ConnectionService, ListenerBinder, PipelinePorts,
};
use crate::{ErrorCode, ProxyError, Result};

pub const DEFAULT_MAX_CONNECTIONS: usize = 500;

#[cfg(not(test))]
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(5);
#[cfg(test)]
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_millis(100);

mod config;
mod facade;
mod factory;
mod lifecycle;
mod runtime;
mod tasks;

pub use config::{ChannelConfig, ChannelId, ProxyConfig, ProxyState, RuntimeSnapshot};
pub use facade::ProxySupervisor;
pub use factory::RuntimeServiceFactory;

use facade::SupervisorCore;
use factory::StaticRuntimeServiceFactory;
use runtime::{
    BoundChannel, CancelOnDrop, Lifecycle, PendingCleanup, PreparedChannel, Runtime, StartedTasks,
    StoppingNotification,
};
use tasks::{
    notify_runtime_stopping, operation_join_error, panic_message, shutdown_runtime, snapshot,
    spawn_listener_task, spawn_watchdog,
};

#[cfg(test)]
mod tests;
