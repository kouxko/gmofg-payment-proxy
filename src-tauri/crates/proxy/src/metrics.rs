//! 供传输层与 pipeline 共享的运行时指标契约。
//!
//! 指标按 runtime epoch 查询，防止重启后把旧连接计入新实例；采集失败返回错误而不是伪造
//! 零值。容量配置是运行时约束，不承担持久化或业务告警职责。

use std::collections::BTreeMap;

use async_trait::async_trait;
use uuid::Uuid;

use crate::{ChannelId, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelRuntimeMetrics {
    pub connected_clients: u32,
    pub request_count: u64,
    pub error_count: u64,
    pub upstream_response_count: u64,
    pub last_upstream_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeMetricsSnapshot {
    pub channels: BTreeMap<ChannelId, ChannelRuntimeMetrics>,
    pub active_sessions: usize,
    pub pending_breakpoints: usize,
    pub logical_memory_bytes: u64,
}

#[async_trait]
pub trait RuntimeMetricsProvider: std::fmt::Debug + Send + Sync {
    async fn configure_capacity(&self, _max_sessions: usize, _max_bytes: u64) -> Result<()> {
        Ok(())
    }

    async fn snapshot(&self, runtime_epoch: Option<Uuid>) -> Result<RuntimeMetricsSnapshot>;
}
