use chrono::{TimeZone, Utc};
use intercept_proxy_application::{
    MessageContentKind, MessageStage, RawHttpHeaderViewModel, UiTone,
};
use intercept_proxy_runtime::Message;

use super::*;

fn request_message() -> Message {
    Message::from_raw_http1_head(
        b"POST /payment?terminal=A%209#fragment HTTP/1.1\r\nX-Trace: first\t\r\nx-trace:second\r\nContent-Type: application/json; charset=utf-8\r\n\r\n",
        bytes::Bytes::from_static(br#"{"amount":100}"#),
    )
    .unwrap()
}

fn content() -> MessageContentViewModel {
    project_message(
        &request_message(),
        DecodedBodyFacts {
            body_text: Some(r#"{"amount":100}"#.into()),
            codec_id: Some("utf-8".into()),
            decode_error: None,
            media_type: Some("application/json".into()),
            charset: Some("utf-8".into()),
            content_kind: MessageContentKind::Json,
        },
    )
}

fn started_session() -> SessionRecord {
    project_started_session(
        SessionStartFacts {
            session_id: Uuid::from_u128(1),
            request_id: "REQ-1".into(),
            started_at: Utc.with_ymd_and_hms(2026, 9, 5, 1, 2, 3).unwrap(),
            terminal_ip: "192.0.2.10".into(),
            channel: ChannelId::new("payments").unwrap(),
            channel_text: "Payment API".into(),
            method: "POST".into(),
            target: "D48".into(),
            runtime_epoch: Uuid::from_u128(2),
            connection_id: Uuid::from_u128(3).to_string(),
            certificate_fingerprint: "AA:BB".into(),
            upstream_host: "example.test".into(),
            app_to_proxy_tls: "mTLS verified".into(),
            request_size_bytes: 14,
        },
        content(),
    )
}

#[test]
fn started_session_projection_populates_initial_summary_and_detail_fields() {
    let record = started_session();
    let summary = &record.detail.summary;

    assert_eq!(summary.session_id, Uuid::from_u128(1));
    assert_eq!(summary.request_id, "REQ-1");
    assert_eq!(
        summary.started_at,
        Utc.with_ymd_and_hms(2026, 9, 5, 1, 2, 3).unwrap()
    );
    assert_eq!(summary.completed_at, None);
    assert_eq!(summary.terminal_ip, "192.0.2.10");
    assert_eq!(summary.channel.as_str(), "payments");
    assert_eq!(summary.channel_text, "Payment API");
    assert_eq!(summary.method, "POST");
    assert_eq!(summary.target, "D48");
    assert_eq!(summary.http_status, None);
    assert_eq!(summary.result, "处理中");
    assert_eq!(summary.ui_tone, UiTone::Info);
    assert_eq!(summary.duration_ms, None);
    assert!(summary.matched_rule_ids.is_empty());
    assert_eq!(summary.request_size_bytes, 14);
    assert_eq!(summary.response_size_bytes, 0);
    assert_eq!(summary.revision, 1);
    assert_eq!(record.detail.runtime_epoch, Uuid::from_u128(2));
    assert_eq!(record.detail.connection_id, Uuid::from_u128(3).to_string());
    assert_eq!(record.detail.certificate_fingerprint, "AA:BB");
    assert_eq!(record.detail.upstream_host, "example.test");
    assert_eq!(record.detail.app_to_proxy_tls, "mTLS verified");
    assert_eq!(
        record.detail.proxy_to_server_tls,
        "上游安全信息等待传输层上报"
    );
    assert_eq!(record.detail.final_action, "处理中");
    assert!(record.detail.timings_ms.is_empty());
    assert!(record.detail.request.is_some());
    assert_eq!(record.detail.response, None);
    assert!(record.detail.rule_trace.is_empty());
}

#[test]
fn message_projection_preserves_wire_headers_and_decoded_fields() {
    let projected = content();

    assert_eq!(
        projected.start_line_bytes,
        b"POST /payment?terminal=A%209#fragment HTTP/1.1"
    );
    assert_eq!(projected.query_string.as_deref(), Some("terminal=A%209"));
    assert_eq!(
        projected.raw_headers[0],
        RawHttpHeaderViewModel {
            name_bytes: b"X-Trace".to_vec(),
            value_bytes: b"first".to_vec(),
            leading_ows_bytes: b" ".to_vec(),
            trailing_ows_bytes: b"\t".to_vec(),
        }
    );
    assert_eq!(projected.raw_headers[1].name_bytes, b"x-trace");
    assert_eq!(projected.headers["X-Trace"], ["first", "second"]);
    assert_eq!(projected.json, Some(serde_json::json!({"amount": 100})));
    assert_eq!(projected.body_bytes, br#"{"amount":100}"#);
    assert_eq!(projected.content_length, 14);
    assert_eq!(projected.codec_id.as_deref(), Some("utf-8"));
    assert_eq!(projected.decode_error, None);
}

#[test]
fn invalid_decoded_json_is_kept_as_text_with_a_visible_error() {
    let mut message = request_message();
    message.body = b"{invalid".to_vec().into();
    let projected = project_message(
        &message,
        DecodedBodyFacts {
            body_text: Some("{invalid".into()),
            codec_id: Some("utf-8".into()),
            decode_error: None,
            media_type: Some("application/json".into()),
            charset: Some("utf-8".into()),
            content_kind: MessageContentKind::Json,
        },
    );

    assert_eq!(projected.body_text.as_deref(), Some("{invalid"));
    assert_eq!(projected.json, None);
    assert!(
        projected
            .decode_error
            .as_deref()
            .unwrap()
            .starts_with("invalid JSON body:")
    );
}

#[test]
fn session_and_capture_projection_copy_every_observed_field() {
    let mut record = started_session();
    let request_rule = Uuid::from_u128(10);
    project_request(
        &mut record,
        content(),
        14,
        RuleOutcomeFacts {
            matched_rule_ids: &[request_rule],
            traces: &["request matched".into()],
        },
    );
    let response_rule = Uuid::from_u128(11);
    let mut response = content();
    response.http_status = Some(202);
    project_response(
        &mut record,
        response,
        8,
        RuleOutcomeFacts {
            matched_rule_ids: &[request_rule, response_rule],
            traces: &["response matched".into()],
        },
    );
    let completed_at = Utc.with_ymd_and_hms(2026, 9, 5, 1, 2, 4).unwrap();
    project_session_completion(
        &mut record,
        completed_at,
        1_234,
        SessionCompletionOutcome::Success,
    );
    let row = project_capture_row(
        &record,
        CaptureRowFacts {
            event_id: 9,
            runtime_epoch: Uuid::from_u128(2),
            occurred_at: completed_at,
            channel_text: "Payment API".into(),
            stage: MessageStage::Terminal,
            result: &record.detail.summary.result,
            ui_tone: record.detail.summary.ui_tone,
            size_bytes: 22,
        },
    );

    assert_eq!(record.detail.summary.revision, 4);
    assert_eq!(record.detail.summary.http_status, Some(202));
    assert_eq!(
        record.detail.summary.matched_rule_ids,
        [request_rule, response_rule]
    );
    assert_eq!(
        record.detail.rule_trace,
        ["request matched", "response matched"]
    );
    assert_eq!(record.detail.summary.completed_at, Some(completed_at));
    assert_eq!(record.detail.summary.duration_ms, Some(1_234));
    assert_eq!(record.detail.timings_ms["total"], 1_234);
    assert_eq!(record.detail.final_action, "响应已返回客户端");
    assert_eq!(row.event_id, 9);
    assert_eq!(row.runtime_epoch, Uuid::from_u128(2));
    assert_eq!(row.session_id, Uuid::from_u128(1));
    assert_eq!(row.terminal_ip, "192.0.2.10");
    assert_eq!(row.channel.as_str(), "payments");
    assert_eq!(row.channel_text, "Payment API");
    assert_eq!(row.stage_text, "终态");
    assert_eq!(row.method, "POST");
    assert_eq!(row.target, "D48");
    assert_eq!(row.http_status, Some(202));
    assert_eq!(row.result, "成功");
    assert_eq!(row.ui_tone, UiTone::Positive);
    assert_eq!(row.duration_ms, Some(1_234));
    assert_eq!(row.matched_rule_ids, [request_rule, response_rule]);
    assert_eq!(row.size_bytes, 22);
}

#[test]
fn dropped_and_failed_projection_keep_existing_rule_order_and_error_semantics() {
    let mut record = started_session();
    let first = Uuid::from_u128(10);
    project_request(
        &mut record,
        content(),
        14,
        RuleOutcomeFacts {
            matched_rule_ids: &[first],
            traces: &["request matched".into()],
        },
    );
    let second = Uuid::from_u128(11);
    project_dropped_response(
        &mut record,
        RuleOutcomeFacts {
            matched_rule_ids: &[first, second],
            traces: &["response dropped".into()],
        },
    );
    project_session_completion(
        &mut record,
        Utc.with_ymd_and_hms(2026, 9, 5, 1, 2, 4).unwrap(),
        100,
        SessionCompletionOutcome::Failure {
            code: "PROXY_STOPPED",
            message: "runtime stopped",
        },
    );

    assert_eq!(record.detail.summary.matched_rule_ids, [first, second]);
    assert_eq!(record.detail.summary.response_size_bytes, 0);
    assert_eq!(record.detail.summary.http_status, None);
    assert_eq!(record.detail.response, None);
    assert_eq!(
        record.detail.rule_trace,
        ["request matched", "response dropped"]
    );
    assert_eq!(
        record.detail.summary.result,
        "Proxy 停止 · PROXY_STOPPED · runtime stopped"
    );
    assert_eq!(record.detail.summary.ui_tone, UiTone::Warning);
    assert_eq!(record.detail.final_action, "runtime stopped");
}

#[test]
fn capture_detail_projection_preserves_payload_and_compacts_the_fingerprint() {
    let mut record = started_session();
    record.detail.certificate_fingerprint = "AA:BB:CC:DD:EE:FF".into();
    record.detail.proxy_to_server_tls = "TLS 1.3 verified".into();
    record.detail.response = Some(content());
    record.detail.timings_ms.insert("total".into(), 42);
    record.detail.rule_trace.push("matched D48".into());
    record.detail.summary.revision = 7;
    let request = record.detail.request.take().unwrap();

    let detail = project_capture_detail(record.detail, request.clone());

    assert_eq!(detail.session_id, Uuid::from_u128(1));
    assert_eq!(detail.request_id, "REQ-1");
    assert_eq!(detail.terminal_ip, "192.0.2.10");
    assert_eq!(detail.certificate_fingerprint_suffix, "CCDDEEFF");
    assert_eq!(detail.upstream_host, "example.test");
    assert_eq!(detail.request, request);
    assert!(detail.response.is_some());
    assert_eq!(detail.tls_summary, "mTLS verified；TLS 1.3 verified");
    assert_eq!(detail.timings_ms["total"], 42);
    assert_eq!(detail.rule_trace, ["matched D48"]);
    assert_eq!(detail.revision, 7);
}
