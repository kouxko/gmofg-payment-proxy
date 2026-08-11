use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::{
    BreakpointId, ChannelId, DisabledReason, MessageStage, PageRequest,
    ResponseAssertionResultViewModel, Revision, RuleId, RuntimeEpoch, SessionId, SortDirection,
    UiTone,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 抓包页提交给 Rust 的筛选、增量游标、排序与分页条件。
pub struct CaptureQuery {
    pub keyword: Option<String>,
    pub terminal_ip: Option<String>,
    pub channel: Option<ChannelId>,
    pub stage: Option<MessageStage>,
    pub result: Option<String>,
    pub rule_id: Option<RuleId>,
    /// 设置后只返回内存中仍保留、且比该游标新的记录。
    pub after_event_id: Option<u64>,
    pub sort: CaptureSort,
    pub direction: SortDirection,
    pub page: PageRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSort {
    OccurredAt,
    TerminalIp,
    Duration,
    Size,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 抓包表格中的轻量行，不包含完整 Payload。
pub struct CaptureRowViewModel {
    pub event_id: u64,
    pub runtime_epoch: RuntimeEpoch,
    pub session_id: SessionId,
    pub occurred_at: DateTime<Utc>,
    pub terminal_ip: String,
    pub channel: ChannelId,
    pub channel_text: String,
    pub stage: MessageStage,
    pub stage_text: String,
    pub method: String,
    pub target: String,
    /// 生成此行时已知的 HTTP 响应码。请求阶段通常为空；响应/终态行可直接显示，
    /// 无需界面先加载完整 Payload。
    pub http_status: Option<u16>,
    pub result: String,
    pub ui_tone: UiTone,
    pub duration_ms: Option<u64>,
    pub matched_rule_ids: Vec<RuleId>,
    pub size_bytes: u64,
    pub breakpoint_id: Option<BreakpointId>,
    pub can_go_to_breakpoint: bool,
    pub breakpoint_disabled_reason: Option<DisabledReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CapturePageViewModel {
    pub rows: Vec<CaptureRowViewModel>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
    pub event_cursor: u64,
    pub oldest_event_id: Option<u64>,
    pub runtime_epoch: Option<RuntimeEpoch>,
    pub snapshot_required: bool,
    pub empty_message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 为无损 HTTP/1 往返保留的一条原始 Header。
/// Header 可能重复、大小写不同或包含有意义的空白，因此不能只用 Map 保存。
pub struct RawHttpHeaderViewModel {
    /// 字段名的精确线上字节，是断点转发时的权威表示；普通 `headers` 只是有损展示投影。
    pub name_bytes: Vec<u8>,
    /// 字段值的精确字节，不含可选空白和 CRLF。
    pub value_bytes: Vec<u8>,
    /// 冒号与实际字段值之间的原始可选空白。
    #[serde(default)]
    pub leading_ows_bytes: Vec<u8>,
    /// 实际字段值之后的原始可选空白。
    #[serde(default)]
    pub trailing_ows_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MessageContentKind {
    Json,
    Xml,
    Text,
    Binary,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 可供界面查看/编辑，同时可无损重建网络报文的内容模型。
pub struct MessageContentViewModel {
    pub http_status: Option<u16>,
    /// 精确起始行字节，避免把展示字符串误作报文重建来源。
    #[serde(default)]
    pub start_line_bytes: Vec<u8>,
    /// 保留名称、值、大小写、重复项和原始顺序的 Header。
    #[serde(default)]
    pub raw_headers: Vec<RawHttpHeaderViewModel>,
    /// 仅供展示和表单编辑的有损分组投影。
    pub headers: BTreeMap<String, Vec<String>>,
    pub body_text: Option<String>,
    pub body_bytes: Vec<u8>,
    #[specta(type = Option<specta_typescript::Unknown<Value>>)]
    pub json: Option<Value>,
    pub content_length: usize,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub content_kind: MessageContentKind,
    #[serde(default)]
    pub codec_id: Option<String>,
    #[serde(default)]
    pub decode_error: Option<String>,
    #[serde(default)]
    pub query_string: Option<String>,
}

impl MessageContentViewModel {
    pub const ENTITY_FIXED_OVERHEAD_BYTES: u64 = 128;

    pub fn logical_bytes(&self) -> u64 {
        let headers = self
            .headers
            .iter()
            .map(|(name, values)| name.len() + values.iter().map(String::len).sum::<usize>())
            .sum::<usize>();
        let raw_headers = self
            .raw_headers
            .iter()
            .map(|header| {
                header.name_bytes.len()
                    + header.leading_ows_bytes.len()
                    + header.value_bytes.len()
                    + header.trailing_ows_bytes.len()
            })
            .sum::<usize>();
        let metadata = [
            &self.media_type,
            &self.charset,
            &self.codec_id,
            &self.decode_error,
            &self.query_string,
        ]
        .into_iter()
        .flatten()
        .map(String::len)
        .sum::<usize>();
        let derived_body = self.body_text.as_ref().map_or(0, String::len)
            + self.json.as_ref().map_or(0, json_logical_bytes);
        Self::ENTITY_FIXED_OVERHEAD_BYTES
            + (self.start_line_bytes.len()
                + headers
                + raw_headers
                + self.body_bytes.len()
                + derived_body
                + metadata) as u64
    }
}

fn json_logical_bytes(value: &Value) -> usize {
    const NODE_OVERHEAD: usize = 16;
    NODE_OVERHEAD
        + match value {
            Value::Null => 0,
            Value::Bool(_) => 1,
            Value::Number(number) => number.to_string().len(),
            Value::String(text) => text.len(),
            Value::Array(values) => values.iter().map(json_logical_bytes).sum(),
            Value::Object(entries) => entries
                .iter()
                .map(|(key, value)| key.len() + json_logical_bytes(value))
                .sum(),
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_message_content_document_defaults_new_http_metadata() {
        let content: MessageContentViewModel = serde_json::from_value(serde_json::json!({
            "http_status": null,
            "start_line_bytes": [],
            "raw_headers": [],
            "headers": {},
            "body_text": null,
            "body_bytes": [0, 255],
            "json": null,
            "content_length": 2
        }))
        .unwrap();

        assert_eq!(content.content_kind, MessageContentKind::Unknown);
        assert!(content.media_type.is_none());
        assert!(content.charset.is_none());
        assert!(content.codec_id.is_none());
        assert!(content.decode_error.is_none());
        assert!(content.query_string.is_none());
        assert_eq!(content.body_bytes, [0, 255]);
    }

    #[test]
    fn logical_bytes_include_text_and_structured_json_projections() {
        let mut content: MessageContentViewModel = serde_json::from_value(serde_json::json!({
            "http_status": null,
            "headers": {},
            "body_text": "D48",
            "body_bytes": [68, 52, 56],
            "json": {"code": "D48"},
            "content_length": 3
        }))
        .unwrap();
        let without_json = {
            content.json = None;
            content.logical_bytes()
        };
        content.json = Some(serde_json::json!({"code": "D48"}));

        assert_eq!(
            without_json,
            MessageContentViewModel::ENTITY_FIXED_OVERHEAD_BYTES + 6,
        );
        assert!(content.logical_bytes() > without_json + "codeD48".len() as u64);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct CaptureDetailViewModel {
    pub session_id: SessionId,
    pub request_id: String,
    pub terminal_ip: String,
    pub certificate_fingerprint_suffix: String,
    pub upstream_host: String,
    pub request: MessageContentViewModel,
    pub response: Option<MessageContentViewModel>,
    pub tls_summary: String,
    pub timings_ms: BTreeMap<String, u64>,
    pub rule_trace: Vec<String>,
    pub extracted_metadata: BTreeMap<String, String>,
    pub response_assertions: Vec<ResponseAssertionResultViewModel>,
    pub revision: Revision,
}
