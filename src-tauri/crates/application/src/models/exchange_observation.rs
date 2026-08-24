//! Exchange 结构化 tracing 投影到 UI/MCP 的连接级运行时模型。
//!
//! 一条记录对应一个 accepted App connection。事件严格按 `Vec` 追加顺序展示，
//! 不引入 message id 或 sequence；运行时报文只存在于有界内存中。

use chrono::{DateTime, Utc};
use intercept_proxy_domain::{ListenerId, ProtocolDirection, WorkspaceId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use uuid::Uuid;

use super::PageRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeProtocol {
    Http,
    Socket,
}

/// Reader/Writer 的强类型网络上下文；HTTP 保留文本 Header/Body，Socket 保留字节。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "protocol", rename_all = "snake_case")]
pub enum ExchangeContext {
    Http { header: String, body: String },
    Socket { bytes: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ExchangeObservationEvent {
    Opened {
        observed_at: DateTime<Utc>,
    },
    Received {
        observed_at: DateTime<Utc>,
        direction: ProtocolDirection,
        context: ExchangeContext,
        /// 协议 Pipeline 提供 Document；透明 Socket chunk 没有 Document。
        #[specta(type = Option<specta_typescript::Unknown<Value>>)]
        document: Option<Value>,
        /// 协议 Reader 固定 Display；透明 Socket 不调用 Display。
        display: Option<String>,
    },
    Sent {
        observed_at: DateTime<Utc>,
        direction: ProtocolDirection,
        context: ExchangeContext,
    },
    Failed {
        observed_at: DateTime<Utc>,
        direction: Option<ProtocolDirection>,
        stage: String,
        context: Option<ExchangeContext>,
        error: String,
    },
    Closed {
        observed_at: DateTime<Utc>,
        outcome: String,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ExchangeObservationRecord {
    pub exchange_id: String,
    pub workspace_id: WorkspaceId,
    pub listener_id: ListenerId,
    pub runtime_epoch: Uuid,
    pub peer_address: String,
    pub protocol: ExchangeProtocol,
    pub events: Vec<ExchangeObservationEvent>,
    /// `true` 表示该连接或更早连接的观测数据因容量不足发生过淘汰。
    pub evidence_evicted: bool,
}

impl ExchangeObservationRecord {
    /// 容量账本使用稳定逻辑字节，不依赖 allocator capacity 或平台指针宽度。
    #[must_use]
    pub fn logical_bytes(&self) -> u64 {
        const RECORD_OVERHEAD: u64 = 160;
        RECORD_OVERHEAD
            + self.exchange_id.len() as u64
            + self.peer_address.len() as u64
            + self.events.iter().map(event_bytes).sum::<u64>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExchangeObservationQuery {
    pub workspace_id: WorkspaceId,
    pub listener_id: Option<ListenerId>,
    pub page: PageRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ExchangeObservationPage {
    pub rows: Vec<ExchangeObservationRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
    /// 当前查询 Workspace 已被整体淘汰、因此无法再返回详情的连接记录数量。
    pub evicted_records: u64,
    /// 应用进程全局未记录事件数：包含生产者队列丢弃、字段解析失败、缺少 opened、
    /// 身份不匹配及内存容量拒绝。无法可信归属的事件不会被猜测到某个 Workspace。
    pub ignored_events: u64,
}

fn event_bytes(event: &ExchangeObservationEvent) -> u64 {
    const EVENT_OVERHEAD: u64 = 64;
    EVENT_OVERHEAD
        + match event {
            ExchangeObservationEvent::Opened { .. } => 0,
            ExchangeObservationEvent::Received {
                context,
                document,
                display,
                ..
            } => {
                context_bytes(context)
                    + document.as_ref().map_or(0, json_bytes)
                    + display.as_ref().map_or(0, |value| value.len() as u64)
            }
            ExchangeObservationEvent::Sent { context, .. } => context_bytes(context),
            ExchangeObservationEvent::Failed {
                stage,
                context,
                error,
                ..
            } => {
                stage.len() as u64 + context.as_ref().map_or(0, context_bytes) + error.len() as u64
            }
            ExchangeObservationEvent::Closed { outcome, error, .. } => {
                outcome.len() as u64 + error.as_ref().map_or(0, |value| value.len() as u64)
            }
        }
}

fn context_bytes(context: &ExchangeContext) -> u64 {
    match context {
        ExchangeContext::Http { header, body } => (header.len() + body.len()) as u64,
        ExchangeContext::Socket { bytes } => bytes.len() as u64,
    }
}

fn json_bytes(value: &Value) -> u64 {
    serde_json::to_vec(value).map_or(u64::MAX, |bytes| bytes.len() as u64)
}
