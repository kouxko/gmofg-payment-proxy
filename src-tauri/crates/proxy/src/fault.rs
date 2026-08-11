//! 网络故障动作原语（`ACTION-001` 至 `ACTION-013`）。
//!
//! 本模块定义动作如何组合以及何时终止后续处理；动作可以故意制造不规范 HTTP，因而
//! “修复”Content-Length 或截断会改变产品语义。延迟类动作必须观察取消令牌，停止代理
//! 时立即退出。

use std::time::Duration;

use crate::message::Message;
use crate::traffic::{JitterScope, TrafficDirection, TrafficSchedule};
use bytes::Bytes;
use http::{HeaderMap, StatusCode};

mod response;

pub use response::{
    apply_response_actions, cancellable_delay, mock_response, project_response_for_observation,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultAction {
    RejectTls,
    DisconnectBeforeUpstream,
    UpstreamConnectTimeout(Duration),
    UpstreamWriteTimeout(Duration),
    UpstreamReadTimeout(Duration),
    DropResponse {
        read_upstream: bool,
    },
    MockResponse {
        status: StatusCode,
        headers: HeaderMap,
        body: Bytes,
    },
    ReplaceBody {
        body: Bytes,
    },
    ContentLengthOffset(i64),
    TruncateResponse(usize),
    Delay(Duration),
    Jitter {
        minimum: Duration,
        maximum: Duration,
        scope: JitterScope,
        seed: u64,
    },
    Throttle {
        bytes_per_second: u64,
        chunk_bytes: usize,
        direction: TrafficDirection,
    },
    Intermittent {
        available: Duration,
        blocked: Duration,
        direction: TrafficDirection,
    },
    DisconnectDuringWrite {
        after_bytes: usize,
        direction: TrafficDirection,
    },
    CustomStatus(StatusCode),
}

#[derive(Debug, Clone)]
pub enum ResponseDisposition {
    Send {
        message: Message,
        schedule: TrafficSchedule,
    },
    Drop,
    Truncate {
        message: Message,
        bytes: usize,
        schedule: TrafficSchedule,
    },
}

#[cfg(test)]
mod tests;
