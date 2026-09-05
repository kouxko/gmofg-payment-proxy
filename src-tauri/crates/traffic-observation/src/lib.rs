//! Pure projections from captured runtime facts into application observation DTOs.
//!
//! Decoding, persistence, publication, runtime lifecycle, and rule transactions stay with the
//! infrastructure pipeline. This crate only assembles already observed facts for display.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    CaptureDetailViewModel, CaptureRowViewModel, ChannelId, MessageContentKind,
    MessageContentViewModel, MessageStage, RawHttpHeaderViewModel, RuleId, SessionDetailViewModel,
    SessionRecord, SessionSummaryViewModel, UiTone,
};
use intercept_proxy_runtime::Message;
use uuid::Uuid;

#[derive(Debug)]
pub struct DecodedBodyFacts {
    pub body_text: Option<String>,
    pub codec_id: Option<String>,
    pub decode_error: Option<String>,
    pub media_type: Option<String>,
    pub charset: Option<String>,
    pub content_kind: MessageContentKind,
}

#[must_use]
pub fn project_message(message: &Message, decoded: DecodedBodyFacts) -> MessageContentViewModel {
    let raw_headers = message
        .headers
        .iter()
        .map(|header| RawHttpHeaderViewModel {
            name_bytes: header.name.to_vec(),
            value_bytes: header.value.to_vec(),
            leading_ows_bytes: header.leading_ows().to_vec(),
            trailing_ows_bytes: header.trailing_ows().to_vec(),
        })
        .collect::<Vec<_>>();
    let headers = display_headers(&raw_headers);
    let mut decode_error = decoded.decode_error;
    let json = if decoded.content_kind == MessageContentKind::Json {
        decoded
            .body_text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .and_then(|text| match serde_json::from_str(text) {
                Ok(value) => Some(value),
                Err(error) => {
                    decode_error = Some(format!("invalid JSON body: {error}"));
                    None
                }
            })
    } else {
        None
    };
    MessageContentViewModel {
        http_status: message.http_status(),
        start_line_bytes: message.start_line.as_bytes().to_vec(),
        raw_headers,
        headers,
        body_text: decoded.body_text,
        body_bytes: message.body.to_vec(),
        json,
        content_length: message.body.len(),
        media_type: decoded.media_type,
        charset: decoded.charset,
        content_kind: decoded.content_kind,
        codec_id: decoded.codec_id,
        decode_error,
        query_string: query_string(&message.start_line),
        protocol: None,
        protocol_failure: None,
    }
}

fn display_headers(raw_headers: &[RawHttpHeaderViewModel]) -> BTreeMap<String, Vec<String>> {
    let mut groups = BTreeMap::<String, (String, Vec<String>)>::new();
    for header in raw_headers {
        let display_name = String::from_utf8_lossy(&header.name_bytes).into_owned();
        let normalized = display_name.to_ascii_lowercase();
        let (_, values) = groups
            .entry(normalized)
            .or_insert_with(|| (display_name, Vec::new()));
        values.push(String::from_utf8_lossy(&header.value_bytes).into_owned());
    }
    groups.into_values().collect()
}

fn query_string(start_line: &str) -> Option<String> {
    let mut fields = start_line.split_ascii_whitespace();
    let first = fields.next()?;
    if first.starts_with("HTTP/") {
        return None;
    }
    let target = fields.next()?;
    let (_, query) = target.split_once('?')?;
    Some(query.split('#').next().unwrap_or_default().to_owned())
}

#[derive(Debug)]
pub struct SessionStartFacts {
    pub session_id: Uuid,
    pub request_id: String,
    pub started_at: DateTime<Utc>,
    pub terminal_ip: String,
    pub channel: ChannelId,
    pub channel_text: String,
    pub method: String,
    pub target: String,
    pub runtime_epoch: Uuid,
    pub connection_id: String,
    pub certificate_fingerprint: String,
    pub upstream_host: String,
    pub app_to_proxy_tls: String,
    pub request_size_bytes: u64,
}

#[must_use]
pub fn project_started_session(
    facts: SessionStartFacts,
    request: MessageContentViewModel,
) -> SessionRecord {
    SessionRecord {
        detail: intercept_proxy_application::SessionDetailViewModel {
            summary: SessionSummaryViewModel {
                session_id: facts.session_id,
                request_id: facts.request_id,
                started_at: facts.started_at,
                completed_at: None,
                terminal_ip: facts.terminal_ip,
                channel: facts.channel,
                channel_text: facts.channel_text,
                method: facts.method,
                target: facts.target,
                http_status: None,
                result: "处理中".into(),
                ui_tone: UiTone::Info,
                duration_ms: None,
                matched_rule_ids: Vec::new(),
                request_size_bytes: facts.request_size_bytes,
                response_size_bytes: 0,
                revision: 1,
            },
            runtime_epoch: facts.runtime_epoch,
            connection_id: facts.connection_id,
            certificate_fingerprint: facts.certificate_fingerprint,
            upstream_host: facts.upstream_host,
            app_to_proxy_tls: facts.app_to_proxy_tls,
            proxy_to_server_tls: "上游安全信息等待传输层上报".into(),
            final_action: "处理中".into(),
            timings_ms: BTreeMap::new(),
            request: Some(request),
            response: None,
            rule_trace: Vec::new(),
        },
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RuleOutcomeFacts<'a> {
    pub matched_rule_ids: &'a [RuleId],
    pub traces: &'a [String],
}

pub fn project_request(
    record: &mut SessionRecord,
    request: MessageContentViewModel,
    request_size_bytes: u64,
    rules: RuleOutcomeFacts<'_>,
) {
    let summary = &mut record.detail.summary;
    summary.matched_rule_ids = rules.matched_rule_ids.to_vec();
    summary.request_size_bytes = request_size_bytes;
    summary.result = "请求已处理".into();
    summary.ui_tone = UiTone::Info;
    summary.revision = summary.revision.saturating_add(1);
    record.detail.request = Some(request);
    record.detail.rule_trace = rules.traces.to_vec();
}

pub fn project_response(
    record: &mut SessionRecord,
    response: MessageContentViewModel,
    response_size_bytes: u64,
    rules: RuleOutcomeFacts<'_>,
) {
    let summary = &mut record.detail.summary;
    append_matched_rule_ids(&mut summary.matched_rule_ids, rules.matched_rule_ids);
    summary.response_size_bytes = response_size_bytes;
    summary.http_status = response.http_status;
    summary.result = "响应已处理".into();
    summary.ui_tone = UiTone::Info;
    summary.revision = summary.revision.saturating_add(1);
    record.detail.response = Some(response);
    record.detail.rule_trace.extend_from_slice(rules.traces);
}

pub fn project_dropped_response(record: &mut SessionRecord, rules: RuleOutcomeFacts<'_>) {
    let summary = &mut record.detail.summary;
    append_matched_rule_ids(&mut summary.matched_rule_ids, rules.matched_rule_ids);
    summary.response_size_bytes = 0;
    summary.http_status = None;
    summary.result = "响应已丢弃".into();
    summary.ui_tone = UiTone::Danger;
    summary.revision = summary.revision.saturating_add(1);
    record.detail.response = None;
    record.detail.rule_trace.extend_from_slice(rules.traces);
}

fn append_matched_rule_ids(existing: &mut Vec<RuleId>, additional: &[RuleId]) {
    for id in additional {
        if !existing.contains(id) {
            existing.push(*id);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SessionCompletionOutcome<'a> {
    Success,
    Failure { code: &'a str, message: &'a str },
}

pub fn project_session_completion(
    record: &mut SessionRecord,
    completed_at: DateTime<Utc>,
    duration_ms: u64,
    outcome: SessionCompletionOutcome<'_>,
) {
    let summary = &mut record.detail.summary;
    summary.completed_at = Some(completed_at);
    summary.duration_ms = Some(duration_ms);
    summary.revision = summary.revision.saturating_add(1);
    match outcome {
        SessionCompletionOutcome::Success => {
            summary.result = "成功".into();
            summary.ui_tone = UiTone::Positive;
            record.detail.final_action = "响应已返回客户端".into();
        }
        SessionCompletionOutcome::Failure { code, message } => {
            summary.result = format!("{} · {code} · {message}", result_text(code));
            summary.ui_tone = result_tone(code);
            record.detail.final_action = message.into();
        }
    }
    record.detail.timings_ms.insert("total".into(), duration_ms);
}

fn result_text(code: &str) -> &'static str {
    match code {
        "UPSTREAM_CONNECT_TIMEOUT" | "UPSTREAM_WRITE_TIMEOUT" | "UPSTREAM_READ_TIMEOUT" => {
            "上游超时"
        }
        "CLIENT_DISCONNECTED" => "App 断开",
        "PROXY_STOPPED" | "FAULT_EXECUTION_CANCELLED" => "Proxy 停止",
        "TLS_HANDSHAKE_FAILED" => "TLS 失败",
        "INCORRECT_CONTENT_LENGTH" => "规则终止",
        "TRUNCATED_RESPONSE" => "截断",
        "FAULT_STREAM_ABORTED" => "弱网断连",
        _ => "内部错误",
    }
}

fn result_tone(code: &str) -> UiTone {
    match code {
        "PROXY_STOPPED" => UiTone::Warning,
        _ => UiTone::Danger,
    }
}

#[derive(Debug)]
pub struct CaptureRowFacts<'a> {
    pub event_id: u64,
    pub runtime_epoch: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub channel_text: String,
    pub stage: MessageStage,
    pub result: &'a str,
    pub ui_tone: UiTone,
    pub size_bytes: u64,
}

#[must_use]
pub fn project_capture_row(
    record: &SessionRecord,
    facts: CaptureRowFacts<'_>,
) -> CaptureRowViewModel {
    let summary = &record.detail.summary;
    CaptureRowViewModel {
        event_id: facts.event_id,
        runtime_epoch: facts.runtime_epoch,
        session_id: summary.session_id,
        occurred_at: facts.occurred_at,
        terminal_ip: summary.terminal_ip.clone(),
        channel: summary.channel.clone(),
        channel_text: facts.channel_text,
        stage: facts.stage,
        stage_text: facts.stage.display_zh().into(),
        method: summary.method.clone(),
        target: summary.target.clone(),
        http_status: summary.http_status,
        result: facts.result.into(),
        ui_tone: facts.ui_tone,
        duration_ms: summary.duration_ms,
        matched_rule_ids: summary.matched_rule_ids.clone(),
        size_bytes: facts.size_bytes,
    }
}

#[must_use]
pub fn project_capture_detail(
    session: SessionDetailViewModel,
    request: MessageContentViewModel,
) -> CaptureDetailViewModel {
    CaptureDetailViewModel {
        session_id: session.summary.session_id,
        request_id: session.summary.request_id,
        terminal_ip: session.summary.terminal_ip,
        certificate_fingerprint_suffix: fingerprint_suffix(&session.certificate_fingerprint),
        upstream_host: session.upstream_host,
        request,
        response: session.response,
        tls_summary: format!(
            "{}；{}",
            session.app_to_proxy_tls, session.proxy_to_server_tls
        ),
        timings_ms: session.timings_ms,
        rule_trace: session.rule_trace,
        revision: session.summary.revision,
    }
}

fn fingerprint_suffix(fingerprint: &str) -> String {
    let compact = fingerprint.replace(':', "");
    let start = compact.len().saturating_sub(8);
    compact[start..].to_owned()
}

#[cfg(test)]
mod tests;
