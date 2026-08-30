//! 规则草稿的领域级非法输入测试。
//!
//! 这些用例只证明“保存前应拒绝什么”，不启动代理、不访问 SQLite，也不能替代
//! 真实设备的规则命中证据。每个测试直接构造领域草稿，以便定位到最小规则语义。

use intercept_proxy_domain::{
    Condition, DropResponseMode, ErrorCode, HttpAction, MatchField, MatchOperator, MessageStage,
    RuleDraft, TerminalAction, validate_rule_draft,
};
use serde_json::json;

fn draft(stage: MessageStage, conditions: Vec<Condition>, actions: Vec<HttpAction>) -> RuleDraft {
    RuleDraft {
        expected_revision: None,
        name: "validation test".into(),
        description: String::new(),
        enabled: true,
        priority: 1,
        created_order: 1,
        channel: None,
        stage,
        conditions,
        actions,
        one_shot: false,
    }
}

fn assert_invalid_field(draft: &RuleDraft, field: &str) {
    let error = validate_rule_draft(draft).expect_err("rule must be rejected");
    assert_eq!(error.code, ErrorCode::RuleInvalid);
    assert!(
        error.field_errors.contains_key(field),
        "missing {field} in {:?}",
        error.field_errors
    );
}

#[test]
fn rejects_action_when_it_is_not_legal_for_the_rule_stage() {
    let invalid = draft(
        MessageStage::Request,
        Vec::new(),
        vec![HttpAction::CustomHttpStatus { status: 503 }],
    );

    assert_invalid_field(&invalid, "actions.0");
}

#[test]
fn rejects_invalid_json_path_in_match_condition() {
    let invalid = draft(
        MessageStage::Request,
        vec![Condition::Http {
            field: MatchField::JsonPath("$.items[]".into()),
            operator: MatchOperator::Equals("item".into()),
        }],
        vec![HttpAction::Pause],
    );

    assert_invalid_field(&invalid, "conditions.0.path");
}

#[test]
fn rejects_invalid_json_path_in_set_json_action() {
    let invalid = draft(
        MessageStage::Request,
        Vec::new(),
        vec![HttpAction::SetJsonField {
            path: "missing-root".into(),
            value: json!("value"),
        }],
    );

    assert_invalid_field(&invalid, "actions.0.path");
}

#[test]
fn rejects_invalid_regular_expression() {
    let invalid = draft(
        MessageStage::Request,
        vec![Condition::Http {
            field: MatchField::TerminalIp,
            operator: MatchOperator::Regex("(".into()),
        }],
        vec![HttpAction::Pause],
    );

    assert_invalid_field(&invalid, "conditions.0.regex");
}

#[test]
fn rejects_invalid_header_name() {
    let invalid = draft(
        MessageStage::Request,
        Vec::new(),
        vec![HttpAction::SetHeader {
            name: "bad header".into(),
            value: "value".into(),
        }],
    );

    assert_invalid_field(&invalid, "actions.0.name");
}

#[test]
fn rejects_header_value_containing_line_breaks() {
    let invalid = draft(
        MessageStage::Request,
        Vec::new(),
        vec![HttpAction::SetHeader {
            name: "x-test".into(),
            value: "value\r\nx-injected: yes".into(),
        }],
    );

    assert_invalid_field(&invalid, "actions.0.value");
}

#[test]
fn rejects_every_header_managed_by_the_forwarding_pipeline() {
    for name in [
        "Content-Length",
        "transfer-encoding",
        "Connection",
        "proxy-connection",
        "Keep-Alive",
        "upgrade",
        "TE",
        "trailer",
    ] {
        let invalid = draft(
            MessageStage::Request,
            Vec::new(),
            vec![HttpAction::SetHeader {
                name: name.into(),
                value: "value".into(),
            }],
        );

        assert_invalid_field(&invalid, "actions.0.name");
    }
}

#[test]
fn rejects_zero_nth_hit() {
    let invalid = draft(
        MessageStage::Request,
        vec![Condition::NthHit { count: 0 }],
        vec![HttpAction::Pause],
    );

    assert_invalid_field(&invalid, "conditions.0.nth_hit");
}

#[test]
fn rejects_zero_duration_for_every_injected_timeout_stage() {
    for action in [
        TerminalAction::UpstreamConnectTimeout { milliseconds: 0 },
        TerminalAction::UpstreamWriteTimeout { milliseconds: 0 },
        TerminalAction::UpstreamReadTimeout { milliseconds: 0 },
    ] {
        let invalid = draft(
            MessageStage::Request,
            Vec::new(),
            vec![HttpAction::Terminal(action)],
        );

        assert_invalid_field(&invalid, "actions.0.milliseconds");
    }
}

#[test]
fn rejects_zero_incorrect_content_length_delta() {
    let invalid = draft(
        MessageStage::Response,
        Vec::new(),
        vec![HttpAction::Terminal(
            TerminalAction::IncorrectContentLength { delta: 0 },
        )],
    );

    assert_invalid_field(&invalid, "actions.0.delta");
}

#[test]
fn rejects_multiple_terminal_actions() {
    let invalid = draft(
        MessageStage::Request,
        Vec::new(),
        vec![
            HttpAction::Terminal(TerminalAction::DropUpstreamResponse {
                mode: DropResponseMode::ReadCompleteResponse,
            }),
            HttpAction::Terminal(TerminalAction::DisconnectBeforeUpstream),
        ],
    );

    assert_invalid_field(&invalid, "actions");
}

#[test]
fn rejects_terminal_action_that_is_not_final() {
    let invalid = draft(
        MessageStage::Request,
        Vec::new(),
        vec![
            HttpAction::Terminal(TerminalAction::DisconnectBeforeUpstream),
            HttpAction::Pause,
        ],
    );

    assert_invalid_field(&invalid, "actions");
}

#[test]
fn domain_preserves_product_owned_mock_body_bytes_without_decoding() {
    let valid = draft(
        MessageStage::Request,
        Vec::new(),
        vec![HttpAction::Terminal(TerminalAction::MockResponse {
            status: 200,
            headers: Vec::new(),
            body_bytes: vec![0x00, 0x82, 0xFF],
        })],
    );

    assert!(validate_rule_draft(&valid).is_ok());
}

#[test]
fn domain_preserves_product_owned_invalid_body_bytes_without_decoding() {
    let valid = draft(
        MessageStage::Response,
        Vec::new(),
        vec![HttpAction::Terminal(TerminalAction::InvalidJson {
            body_bytes: vec![0x00, 0x82, 0xFF],
        })],
    );

    assert!(validate_rule_draft(&valid).is_ok());
}
