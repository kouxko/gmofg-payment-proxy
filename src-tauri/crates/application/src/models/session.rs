use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    ChannelId, MessageContentViewModel, ResponseAssertionId, Revision, RuleId, RuntimeEpoch,
    SessionId, SortDirection, UiTone,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 会话页提交给 Rust 的筛选、时间范围和排序条件。
pub struct SessionQuery {
    pub keyword: Option<String>,
    pub terminal_ip: Option<String>,
    pub channel: Option<ChannelId>,
    pub result: Option<String>,
    pub rule_id: Option<RuleId>,
    pub started_from: Option<DateTime<Utc>>,
    pub started_to: Option<DateTime<Utc>>,
    pub sort: SessionSort,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionSort {
    StartedAt,
    TerminalIp,
    Duration,
    RequestSize,
    ResponseSize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionSummaryViewModel {
    pub session_id: SessionId,
    pub request_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub terminal_ip: String,
    pub channel: ChannelId,
    pub channel_text: String,
    pub method: String,
    pub target: String,
    pub http_status: Option<u16>,
    pub result: String,
    pub ui_tone: UiTone,
    pub duration_ms: Option<u64>,
    pub matched_rule_ids: Vec<RuleId>,
    pub request_size_bytes: u64,
    pub response_size_bytes: u64,
    pub pending_breakpoint: bool,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionListViewModel {
    pub items: Vec<SessionSummaryViewModel>,
    pub total: usize,
    pub empty_message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 会话详情；完整请求/响应只在用户打开详情时返回。
pub struct SessionDetailViewModel {
    pub summary: SessionSummaryViewModel,
    pub runtime_epoch: RuntimeEpoch,
    pub connection_id: String,
    pub certificate_fingerprint: String,
    pub upstream_host: String,
    pub app_to_proxy_tls: String,
    pub proxy_to_server_tls: String,
    pub final_action: String,
    pub timings_ms: BTreeMap<String, u64>,
    pub request: Option<MessageContentViewModel>,
    pub response: Option<MessageContentViewModel>,
    pub rule_trace: Vec<String>,
    /// 对最终响应执行的通用断言结果；失败只影响会话结论，不篡改线上响应。
    #[serde(default)]
    pub response_assertions: Vec<ResponseAssertionResultViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ResponseAssertionResultViewModel {
    pub assertion_id: ResponseAssertionId,
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 内存仓储实际保存的会话记录，并提供精确逻辑容量计算。
pub struct SessionRecord {
    pub detail: SessionDetailViewModel,
    pub breakpoint_draft: Option<MessageContentViewModel>,
}

impl SessionRecord {
    pub const ENTITY_FIXED_OVERHEAD_BYTES: u64 = 256;

    pub fn id(&self) -> SessionId {
        self.detail.summary.session_id
    }

    pub fn is_pending(&self) -> bool {
        self.detail.summary.pending_breakpoint
    }

    pub fn logical_bytes(&self) -> u64 {
        let summary = &self.detail.summary;
        let fixed_strings = summary.request_id.len()
            + summary.terminal_ip.len()
            + summary.channel_text.len()
            + summary.method.len()
            + summary.target.len()
            + summary.result.len()
            + self.detail.connection_id.len()
            + self.detail.certificate_fingerprint.len()
            + self.detail.upstream_host.len()
            + self.detail.app_to_proxy_tls.len()
            + self.detail.proxy_to_server_tls.len()
            + self.detail.final_action.len();
        let rule_trace_bytes =
            serde_json::to_vec(&self.detail.rule_trace).map_or(0, |bytes| bytes.len());
        let policy_result_bytes =
            serde_json::to_vec(&self.detail.response_assertions).map_or(0, |bytes| bytes.len());
        let messages = self
            .detail
            .request
            .as_ref()
            .map_or(0, MessageContentViewModel::logical_bytes)
            + self
                .detail
                .response
                .as_ref()
                .map_or(0, MessageContentViewModel::logical_bytes)
            + self
                .breakpoint_draft
                .as_ref()
                .map_or(0, MessageContentViewModel::logical_bytes);
        Self::ENTITY_FIXED_OVERHEAD_BYTES
            + fixed_strings as u64
            + rule_trace_bytes as u64
            + policy_result_bytes as u64
            + messages
    }
}
