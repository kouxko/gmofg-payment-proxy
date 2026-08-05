#[test]
fn validates_tls_match_scope_and_truncation_boundary() {
    let tls_rule = draft(
        MessageStage::TlsHandshake,
        vec![MatchCondition::Field {
            field: MatchField::TerminalIp,
            operator: MatchOperator::Equals("10.0.0.8".into()),
        }],
        vec![RuleAction::Terminal(TerminalAction::RejectTlsHandshake)],
    );
    assert!(validate_rule_draft(&tls_rule).is_err());

    let truncate = TerminalAction::TruncateResponse { bytes: 2 };
    assert!(truncate.validate_for_body(3).is_ok());
    assert!(truncate.validate_for_body(2).is_err());
    assert!(
        TerminalAction::TruncateResponse { bytes: 0 }
            .validate_for_body(1)
            .is_ok()
    );
    assert!(
        TerminalAction::TruncateResponse { bytes: 0 }
            .validate_for_body(0)
            .is_err()
    );
}

#[test]
fn validates_json_paths_and_headers_without_assuming_a_product_codec() {
    let invalid = draft(
        MessageStage::Response,
        vec![MatchCondition::Field {
            field: MatchField::JsonPath("$.items[]".into()),
            operator: MatchOperator::Equals("x".into()),
        }],
        vec![
            RuleAction::SetJsonField {
                path: "missing_root.field".into(),
                value: json!("🧪"),
            },
            RuleAction::ReplaceBodyText("emoji 🧪".into()),
            RuleAction::SetHeader {
                name: "content-length".into(),
                value: "12\r\nx-injected: yes".into(),
            },
            RuleAction::Terminal(TerminalAction::MockResponse {
                status: 200,
                headers: vec![("bad header".into(), "value".into())],
                body_bytes: vec![0x82],
            }),
        ],
    );
    let error = validate_rule_draft(&invalid).expect_err("all invalid fields fail closed");
    for field in [
        "conditions.0.path",
        "actions.0.path",
        "actions.2.name",
        "actions.2.value",
        "actions.3.headers.0.name",
    ] {
        assert!(
            error.field_errors.contains_key(field),
            "missing field error for {field}: {:?}",
            error.field_errors
        );
    }

    assert!(
        validate_rule_draft(&draft(
            MessageStage::Response,
            Vec::new(),
            vec![RuleAction::Terminal(TerminalAction::InvalidJson {
                body_bytes: vec![0x00, 0xFF],
            })],
        ))
        .is_ok(),
        "body bytes are interpreted by the injected product codec outside the domain"
    );
}

// RULE-012
#[test]
fn warns_when_higher_priority_terminal_rule_can_shadow_lower_rule() {
    let epoch = RuntimeEpoch::new();
    let higher = Rule::create(draft(
        MessageStage::Request,
        Vec::new(),
        vec![RuleAction::Terminal(
            TerminalAction::DisconnectBeforeUpstream,
        )],
    ))
    .unwrap();
    let mut lower = Rule::create(draft(
        MessageStage::Request,
        vec![MatchCondition::NthHit(2)],
        vec![RuleAction::Pause],
    ))
    .unwrap();
    lower.priority = 20;
    let warnings = RuleEngine::new(epoch, vec![lower, higher]).conflict_warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, ErrorCode::RuleConflictWarning);
}

#[test]
fn validates_weak_network_parameter_boundaries_and_stages() {
    let valid = draft(
        MessageStage::Request,
        Vec::new(),
        vec![
            RuleAction::Jitter {
                minimum_milliseconds: 10,
                maximum_milliseconds: 20,
                scope: JitterScope::PerChunk,
            },
            RuleAction::Throttle {
                bytes_per_second: 1,
                chunk_bytes: MAX_TRAFFIC_CHUNK_BYTES,
                direction: TrafficDirection::Upstream,
            },
            RuleAction::Intermittent {
                available_milliseconds: 1,
                blocked_milliseconds: MAX_TOTAL_DELAY_MS,
                direction: TrafficDirection::Upstream,
            },
            RuleAction::Terminal(TerminalAction::DisconnectDuringUpstreamWrite { after_bytes: 1 }),
        ],
    );
    assert!(validate_rule_draft(&valid).is_ok());

    let invalid = draft(
        MessageStage::Request,
        Vec::new(),
        vec![
            RuleAction::Jitter {
                minimum_milliseconds: 2,
                maximum_milliseconds: 1,
                scope: JitterScope::BeforeMessage,
            },
            RuleAction::Throttle {
                bytes_per_second: 0,
                chunk_bytes: MAX_TRAFFIC_CHUNK_BYTES + 1,
                direction: TrafficDirection::Upstream,
            },
            RuleAction::Intermittent {
                available_milliseconds: 0,
                blocked_milliseconds: MAX_TOTAL_DELAY_MS + 1,
                direction: TrafficDirection::Upstream,
            },
            RuleAction::Terminal(TerminalAction::DisconnectDuringDownstreamWrite {
                after_bytes: 1,
            }),
        ],
    );
    let error = validate_rule_draft(&invalid).expect_err("weak network bounds");
    for field in [
        "actions.0.minimum_milliseconds",
        "actions.1.bytes_per_second",
        "actions.1.chunk_bytes",
        "actions.2.available_milliseconds",
        "actions.2.blocked_milliseconds",
        "actions.3",
    ] {
        assert!(
            error.field_errors.contains_key(field),
            "missing field error for {field}: {:?}",
            error.field_errors
        );
    }
}

#[test]
fn mid_body_disconnect_offset_is_validated_against_runtime_body() {
    let upstream = TerminalAction::DisconnectDuringUpstreamWrite { after_bytes: 3 };
    assert!(upstream.validate_for_body(4).is_ok());
    assert!(upstream.validate_for_body(3).is_err());
    let downstream = TerminalAction::DisconnectDuringDownstreamWrite { after_bytes: 0 };
    assert!(downstream.validate_for_body(1).is_ok());
    assert!(downstream.validate_for_body(0).is_err());
}
