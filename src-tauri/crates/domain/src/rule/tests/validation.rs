#[test]
fn validates_regex_delay_terminal_order_phase_and_action_parameters() {
    let invalid = draft(
        MessageStage::Request,
        vec![MatchCondition::Field {
            field: MatchField::TerminalIp,
            operator: MatchOperator::Regex("(".into()),
        }.into()],
        vec![
            RuleAction::Delay {
                milliseconds: MAX_TOTAL_DELAY_MS + 1,
            },
            RuleAction::Terminal(TerminalAction::IncorrectContentLength { delta: 0 }),
            RuleAction::Pause,
        ],
    );
    let error = validate_rule_draft(&invalid).unwrap_err();
    assert_eq!(error.code, ErrorCode::RuleInvalid);
    assert!(error.field_errors.len() >= 4);
}

// RULE-009~011, ACTION-001~013, TEST-RULE:
// every UI action has one explicit legal phase contract.
#[test]
#[allow(clippy::too_many_lines)]
fn validates_every_action_against_its_exact_stage_contract() {
    let cases = vec![
        (
            RuleAction::SetJsonField {
                path: "$.value".into(),
                value: json!(1),
            },
            vec![MessageStage::Request, MessageStage::Response],
        ),
        (
            RuleAction::ReplaceBodyText("本文".into()),
            vec![MessageStage::Request, MessageStage::Response],
        ),
        (
            RuleAction::SetHeader {
                name: "x-test".into(),
                value: "enabled".into(),
            },
            vec![MessageStage::Request, MessageStage::Response],
        ),
        (
            RuleAction::Delay { milliseconds: 1 },
            vec![MessageStage::Request, MessageStage::Response],
        ),
        (
            RuleAction::Jitter {
                minimum_milliseconds: 1,
                maximum_milliseconds: 2,
                scope: JitterScope::PerChunk,
            },
            vec![MessageStage::Request, MessageStage::Response],
        ),
        (
            RuleAction::Throttle {
                bytes_per_second: 1_024,
                chunk_bytes: 256,
                direction: TrafficDirection::Upstream,
            },
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Throttle {
                bytes_per_second: 1_024,
                chunk_bytes: 256,
                direction: TrafficDirection::Downstream,
            },
            vec![MessageStage::Response],
        ),
        (
            RuleAction::Intermittent {
                available_milliseconds: 10,
                blocked_milliseconds: 10,
                direction: TrafficDirection::Upstream,
            },
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Intermittent {
                available_milliseconds: 10,
                blocked_milliseconds: 10,
                direction: TrafficDirection::Downstream,
            },
            vec![MessageStage::Response],
        ),
        (
            RuleAction::Pause,
            vec![MessageStage::Request, MessageStage::Response],
        ),
        (
            RuleAction::CustomHttpStatus { status: 503 },
            vec![MessageStage::Response],
        ),
        (
            RuleAction::Terminal(TerminalAction::RejectTlsHandshake),
            vec![MessageStage::TlsHandshake],
        ),
        (
            RuleAction::Terminal(TerminalAction::DisconnectBeforeUpstream),
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Terminal(TerminalAction::UpstreamConnectTimeout { milliseconds: 1 }),
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Terminal(TerminalAction::UpstreamWriteTimeout { milliseconds: 1 }),
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Terminal(TerminalAction::UpstreamReadTimeout { milliseconds: 1 }),
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Terminal(TerminalAction::DropUpstreamResponse {
                mode: DropResponseMode::ReadCompleteResponse,
            }),
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Terminal(TerminalAction::DropUpstreamResponse {
                mode: DropResponseMode::CloseAfterRequestWrite,
            }),
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Terminal(TerminalAction::MockResponse {
                status: 200,
                headers: vec![("x-mock".into(), "true".into())],
                body_bytes: b"{}".to_vec(),
            }),
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Terminal(TerminalAction::InvalidJson {
                body_bytes: b"{".to_vec(),
            }),
            vec![MessageStage::Response],
        ),
        (
            RuleAction::Terminal(TerminalAction::IncorrectContentLength { delta: 1 }),
            vec![MessageStage::Response],
        ),
        (
            RuleAction::Terminal(TerminalAction::TruncateResponse { bytes: 0 }),
            vec![MessageStage::Response],
        ),
        (
            RuleAction::Terminal(TerminalAction::DisconnectDuringUpstreamWrite {
                after_bytes: 0,
            }),
            vec![MessageStage::Request],
        ),
        (
            RuleAction::Terminal(TerminalAction::DisconnectDuringDownstreamWrite {
                after_bytes: 0,
            }),
            vec![MessageStage::Response],
        ),
    ];
    let all_stages = [
        MessageStage::TlsHandshake,
        MessageStage::Request,
        MessageStage::Response,
    ];

    for (action, legal_stages) in cases {
        for stage in all_stages {
            let result = validate_rule_draft(&draft(stage, Vec::new(), vec![action.clone()]));
            assert_eq!(
                result.is_ok(),
                legal_stages.contains(&stage),
                "unexpected phase contract for {action:?} at {stage:?}: {result:?}"
            );
        }
    }
}

// RULE-006~007, ENGINE-007, TEST-RULE:
// NthHit identity is exactly the pair (terminal IP, certificate fingerprint).
#[test]
fn nth_hit_counter_is_scoped_by_both_terminal_identity_components() {
    let epoch = RuntimeEpoch::new();
    let rule = Rule::create(draft(
        MessageStage::Request,
        vec![Condition::NthHit { count: 2 }],
        vec![RuleAction::Pause],
    ))
    .expect("valid nth-hit rule");
    let base = TerminalIdentity {
        source_ip: "10.0.0.8".into(),
        certificate_sha256: "cert-a".into(),
    };
    let same_ip_other_cert = TerminalIdentity {
        source_ip: base.source_ip.clone(),
        certificate_sha256: "cert-b".into(),
    };
    let other_ip_same_cert = TerminalIdentity {
        source_ip: "10.0.0.9".into(),
        certificate_sha256: base.certificate_sha256.clone(),
    };
    let mut engine = RuleEngine::new(epoch, vec![rule]);

    for identity in [&base, &same_ip_other_cert, &other_ip_same_cert] {
        assert!(
            !engine
                .evaluate(&context(epoch, identity, None), Utc::now())
                .traces[0]
                .matched
        );
    }
    for identity in [&base, &same_ip_other_cert, &other_ip_same_cert] {
        assert!(
            engine
                .evaluate(&context(epoch, identity, None), Utc::now())
                .traces[0]
                .matched,
            "each IP + certificate pair must maintain an independent count"
        );
    }
}

#[test]
fn tls_rejection_preserves_nth_hit_semantics() {
    let epoch = RuntimeEpoch::new();
    let rule = Rule::create(draft(
        MessageStage::TlsHandshake,
        vec![Condition::NthHit { count: 2 }],
        vec![RuleAction::Terminal(TerminalAction::RejectTlsHandshake)],
    ))
    .expect("TLS NthHit is a valid pre-HTTP condition");
    let identity = TerminalIdentity {
        source_ip: "10.0.34.94".into(),
        certificate_sha256: "terminal-cert".into(),
    };
    let mut engine = RuleEngine::new(epoch, vec![rule]);
    let tls_context = MatchContext {
        runtime_epoch: epoch,
        channel: ChannelId::new("beta").unwrap(),
        stage: MessageStage::TlsHandshake,
        terminal: &identity,
        path_or_request_type: None,
        json_body: None,
    };

    let first = engine.evaluate(&tls_context, Utc::now());
    assert!(!first.traces[0].matched);
    assert!(first.composed_actions.is_empty());
    assert!(first.terminal_action.is_none());

    let second = engine.evaluate(&tls_context, Utc::now());
    assert!(second.traces[0].matched);
    assert!(matches!(
        second.composed_actions.as_slice(),
        [RuleAction::Terminal(TerminalAction::RejectTlsHandshake)]
    ));
    assert!(matches!(
        second.terminal_action,
        Some(TerminalAction::RejectTlsHandshake)
    ));

    let third = engine.evaluate(&tls_context, Utc::now());
    assert!(!third.traces[0].matched);
    assert!(third.composed_actions.is_empty());
    assert!(third.terminal_action.is_none());
}

// ACTION-001, ACTION-011
