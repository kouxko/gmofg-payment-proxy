use super::*;
use chrono::Utc;
use intercept_proxy_product_api::ProductError;
use uuid::Uuid;

use crate::{BreakpointState, BreakpointSummaryViewModel, ChannelId, UiTone};

#[derive(Debug)]
struct TestBodyCodec;

impl BodyCodec for TestBodyCodec {
    fn id(&self) -> &'static str {
        "test-prefix"
    }

    fn name(&self) -> &'static str {
        "Test Prefix"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        let body = bytes.strip_prefix(&[0xFF]).unwrap_or(bytes);
        String::from_utf8(body.to_vec())
            .map_err(|error| ProductError::new("BODY_DECODE_FAILED", error.to_string()))
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        if text.contains('🧪') {
            return Err(ProductError::new(
                "BODY_ENCODE_FAILED",
                "test codec rejects the marker character",
            ));
        }
        let mut encoded = vec![0xFF];
        encoded.extend_from_slice(text.as_bytes());
        Ok(encoded)
    }
}

fn validator() -> BreakpointValidator {
    BreakpointValidator::new(Arc::new(TestBodyCodec))
}

fn message(text: &str) -> MessageContentViewModel {
    MessageContentViewModel {
        http_status: None,
        start_line_bytes: Vec::new(),
        raw_headers: Vec::new(),
        headers: BTreeMap::from([("content-type".into(), vec!["application/json".into()])]),
        body_text: Some(text.into()),
        body_bytes: text.as_bytes().to_vec(),
        json: None,
        content_length: text.len(),
        media_type: Some("application/json".into()),
        charset: None,
        content_kind: crate::MessageContentKind::Json,
        codec_id: Some("test-prefix".into()),
        decode_error: None,
        query_string: None,
    }
}

fn detail(stage: MessageStage) -> BreakpointDetailViewModel {
    let original = message(r#"{"amount":100}"#);
    BreakpointDetailViewModel {
        summary: BreakpointSummaryViewModel {
            breakpoint_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            runtime_epoch: Uuid::new_v4(),
            stage,
            title: String::new(),
            terminal_ip: "127.0.0.1".into(),
            channel: ChannelId::new("alpha").unwrap(),
            channel_text: "Alpha".into(),
            method: "POST".into(),
            target: "/pay".into(),
            waiting_since: Utc::now(),
            certificate_fingerprint_suffix: "1234".into(),
            state: BreakpointState::Pending,
            state_text: String::new(),
            ui_tone: UiTone::Warning,
            revision: 1,
        },
        original: original.clone(),
        effective: original,
        can_resolve: true,
        resolve_disabled_reason: None,
        available_actions: Vec::new(),
    }
}

#[test]
fn format_json_uses_injected_product_codec_and_recalculates_length() {
    let detail = detail(MessageStage::Request);
    let draft = BreakpointDraft {
        breakpoint_id: detail.summary.breakpoint_id,
        expected_revision: 1,
        message: message(r#"{"result":"承認"}"#),
    };
    let formatted = validator().format_json(draft).expect("format succeeds");
    assert_eq!(
        formatted.message.content_length,
        formatted.message.body_bytes.len()
    );
    assert_ne!(
        formatted.message.body_bytes,
        formatted.message.body_text.unwrap().into_bytes()
    );
}

#[test]
fn format_json_rejects_non_json_media_types() {
    let detail = detail(MessageStage::Request);
    let mut xml = message("<result>ok</result>");
    xml.media_type = Some("application/xml".into());
    xml.content_kind = crate::MessageContentKind::Xml;
    let error = validator()
        .format_json(BreakpointDraft {
            breakpoint_id: detail.summary.breakpoint_id,
            expected_revision: 1,
            message: xml,
        })
        .expect_err("XML must not be treated as JSON");

    assert_eq!(error.view_model.code, "JSON_MEDIA_TYPE_REQUIRED");
}

#[test]
fn rejects_unencodable_text_and_missing_truncate_position() {
    let detail = detail(MessageStage::Response);
    let draft = BreakpointDraft {
        breakpoint_id: detail.summary.breakpoint_id,
        expected_revision: 1,
        message: message(r#"{"value":"🧪"}"#),
    };
    assert!(
        !validator()
            .validate(&detail, &draft)
            .expect("validation")
            .valid
    );

    let decision = BreakpointDecision {
        breakpoint_id: detail.summary.breakpoint_id,
        expected_revision: 1,
        kind: BreakpointDecisionKind::Truncate,
        message: None,
        delay_ms: None,
        http_status: None,
        content_length_delta: None,
        truncate_at: None,
    };
    assert!(
        !validator()
            .validate_decision(&detail, &decision)
            .expect("validation")
            .valid
    );
    assert!(
        validator()
            .validate_decision(
                &detail,
                &BreakpointDecision {
                    truncate_at: Some(0),
                    ..decision
                },
            )
            .expect("zero position is valid")
            .valid
    );
}

#[test]
fn rejects_start_line_edits_and_status_codes_above_the_http_contract() {
    let detail = detail(MessageStage::Response);
    let mut edited = detail.effective.clone();
    edited.start_line_bytes = b"HTTP/1.1 200 OK\r\nX-Injected: value".to_vec();
    let decision = BreakpointDecision {
        breakpoint_id: detail.summary.breakpoint_id,
        expected_revision: detail.summary.revision,
        kind: BreakpointDecisionKind::ForwardModified,
        message: Some(edited),
        delay_ms: None,
        http_status: None,
        content_length_delta: None,
        truncate_at: None,
    };
    let validation = validator()
        .validate_decision(&detail, &decision)
        .expect("validation");
    assert!(!validation.valid);
    assert!(
        validation
            .field_errors
            .contains_key("message.start_line_bytes")
    );

    for status in [600, 700, 999] {
        let validation = validator()
            .validate_decision(
                &detail,
                &BreakpointDecision {
                    kind: BreakpointDecisionKind::CustomHttpStatus,
                    message: None,
                    http_status: Some(status),
                    ..decision.clone()
                },
            )
            .expect("status validation");
        assert!(!validation.valid, "{status} must be rejected");
        assert!(validation.field_errors.contains_key("http_status"));
    }
}
