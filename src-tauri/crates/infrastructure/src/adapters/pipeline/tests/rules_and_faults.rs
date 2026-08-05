#[test]
fn nth_hit_retry_preserves_prior_count_without_double_counting_current_message() {
    let rule = view_to_domain_rule({
        let mut view = one_shot_delay_rule();
        view.draft.conditions =
            vec![intercept_proxy_application::RuleCondition::NthHit { count: 2 }];
        view.draft.one_shot = false;
        view
    })
    .expect("rule");
    let rules = Arc::new(ConflictOnceRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        conflict_once: AtomicBool::new(true),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(128)),
        Arc::new(CaptureRepositoryAdapter::default()),
    );
    let epoch = Uuid::new_v4();
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    let message = request_message(r#"{"amount":100}"#);
    let body_codec = test_body_codec();

    let first = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            body_codec.as_ref(),
        )
        .expect("first evaluation");
    assert!(
        first.actions.is_empty(),
        "the first NthHit(2) evaluation only advances the in-memory counter"
    );
    assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 0);

    let second = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            body_codec.as_ref(),
        )
        .expect("second evaluation retries after the injected conflict");
    assert_eq!(
        second.actions,
        vec![RuleAction::Delay { milliseconds: 25 }],
        "the same second request must still hit after rollback and re-evaluation"
    );
    assert_eq!(
        rules.commit_attempts.load(AtomicOrdering::Acquire),
        2,
        "one conflicting CAS and one successful retry are expected"
    );
    {
        let persisted = rules.snapshot.lock();
        assert_eq!(persisted.rules[0].hit_count, 1);
        assert_eq!(
            persisted.collection_revision, 2,
            "the external advance and successful retry each advance once"
        );
    }

    let third = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            body_codec.as_ref(),
        )
        .expect("third evaluation");
    assert!(
        third.actions.is_empty(),
        "the retry must not count the second request twice"
    );
    assert_eq!(
        rules.snapshot.lock().rules[0].hit_count,
        1,
        "only the exact second hit executes and persists"
    );
}

#[test]
fn tls_handshake_policy_matches_the_peer_under_current_verification() {
    let fingerprint = "11:22:33:44";
    let pipeline = adapter(vec![tls_fingerprint_reject_rule(fingerprint)], 10);
    let epoch = Uuid::new_v4();
    let mut context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    context.tls_peer = None;
    let matching_peer = TlsPeerIdentity {
        sha256_fingerprint: fingerprint.into(),
        subject_summary: "CN=blocked".into(),
    };
    assert!(
        pipeline
            .reject_tls_handshake(&context, &matching_peer)
            .expect("policy")
    );

    let other_peer = TlsPeerIdentity {
        sha256_fingerprint: "AA:BB".into(),
        subject_summary: "CN=allowed".into(),
    };
    assert!(
        !pipeline
            .reject_tls_handshake(&context, &other_peer)
            .expect("policy")
    );
}

#[test]
fn rule_mutations_use_injected_codec_and_preserve_action_order() {
    let body_codec = test_body_codec();
    let mut message = request_message(r#"{"payment":{"amount":100}}"#);
    let actions = vec![
        RuleAction::SetJsonField {
            path: "$.payment.amount".into(),
            value: json!(200),
        },
        RuleAction::SetHeader {
            name: "x-test".into(),
            value: "yes".into(),
        },
        RuleAction::Delay { milliseconds: 25 },
        RuleAction::Pause,
    ];
    let (faults, pause) =
        apply_rule_actions(body_codec.as_ref(), &mut message, &actions, 42).expect("apply");
    assert!(pause);
    assert_eq!(faults, vec![FaultAction::Delay(Duration::from_millis(25))]);
    assert_eq!(
        decode_json(body_codec.as_ref(), &message.body).expect("json")["payment"]["amount"],
        200
    );
    assert_eq!(message.declared_content_length(), Some(message.body.len()));
    assert_eq!(header_value(&message, "x-test").as_deref(), Some("yes"));

    let mock = map_terminal_action(&TerminalAction::MockResponse {
        status: 503,
        headers: vec![("x-mock".into(), "enabled".into())],
        body_bytes: br#"{"mock":true}"#.to_vec(),
    })
    .expect("mock");
    let FaultAction::MockResponse {
        status,
        headers,
        body,
    } = mock
    else {
        panic!("expected mock action");
    };
    assert_eq!(status.as_u16(), 503);
    assert_eq!(headers["x-mock"], "enabled");
    assert_eq!(body, Bytes::from_static(br#"{"mock":true}"#));
}

// RULE-008~009, ACTION-012~013, MESSAGE-004~006, TEST-RULE:
// later body/header mutations win and Rust rebuilds Content-Length exactly once.
#[test]
fn body_replacement_and_repeated_header_updates_preserve_action_order() {
    let body_codec = test_body_codec();
    let mut message = request_message(r#"{"original":true}"#);
    message
        .headers
        .push(RawHeader::new(b"x-test".to_vec(), b"old".to_vec()));
    let actions = vec![
        RuleAction::ReplaceBodyText("最初".into()),
        RuleAction::ReplaceBodyText("最終".into()),
        RuleAction::SetHeader {
            name: "x-test".into(),
            value: "first".into(),
        },
        RuleAction::SetHeader {
            name: "x-test".into(),
            value: "last".into(),
        },
    ];

    let (faults, pause) = apply_rule_actions(body_codec.as_ref(), &mut message, &actions, 42)
        .expect("apply mutations");
    assert!(faults.is_empty());
    assert!(!pause);
    assert_eq!(
        decode_body(body_codec.as_ref(), &message.body).expect("decode"),
        "最終"
    );
    assert_eq!(message.declared_content_length(), Some(message.body.len()));
    assert_eq!(header_value(&message, "x-test").as_deref(), Some("last"));
    assert_eq!(
        message
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case(b"x-test"))
            .count(),
        1,
        "SetHeader replaces all earlier values for the same header"
    );
}

// ACTION-001~011, TEST-FAULT:
// all terminal domain actions map to one explicit transport disposition.
#[test]
fn every_terminal_action_maps_to_the_expected_transport_fault() {
    let cases = vec![
        (TerminalAction::RejectTlsHandshake, FaultAction::RejectTls),
        (
            TerminalAction::DisconnectBeforeUpstream,
            FaultAction::DisconnectBeforeUpstream,
        ),
        (
            TerminalAction::UpstreamConnectTimeout { milliseconds: 1 },
            FaultAction::UpstreamConnectTimeout(Duration::from_millis(1)),
        ),
        (
            TerminalAction::UpstreamWriteTimeout { milliseconds: 1 },
            FaultAction::UpstreamWriteTimeout(Duration::from_millis(1)),
        ),
        (
            TerminalAction::UpstreamReadTimeout { milliseconds: 1 },
            FaultAction::UpstreamReadTimeout(Duration::from_millis(1)),
        ),
        (
            TerminalAction::DropUpstreamResponse {
                mode: DropResponseMode::ReadCompleteResponse,
            },
            FaultAction::DropResponse {
                read_upstream: true,
            },
        ),
        (
            TerminalAction::DropUpstreamResponse {
                mode: DropResponseMode::CloseAfterRequestWrite,
            },
            FaultAction::DropResponse {
                read_upstream: false,
            },
        ),
        (
            TerminalAction::InvalidJson {
                body_bytes: b"{".to_vec(),
            },
            FaultAction::ReplaceBody {
                body: Bytes::from_static(b"{"),
            },
        ),
        (
            TerminalAction::IncorrectContentLength { delta: -1 },
            FaultAction::ContentLengthOffset(-1),
        ),
        (
            TerminalAction::TruncateResponse { bytes: 2 },
            FaultAction::TruncateResponse(2),
        ),
    ];
    for (domain, expected) in cases {
        assert_eq!(
            map_terminal_action(&domain).expect("map terminal action"),
            expected,
            "unexpected transport mapping for {domain:?}"
        );
    }

    let mock = map_terminal_action(&TerminalAction::MockResponse {
        status: 202,
        headers: vec![("x-mock".into(), "yes".into())],
        body_bytes: br#"{"mock":true}"#.to_vec(),
    })
    .expect("map mock response");
    let FaultAction::MockResponse {
        status,
        headers,
        body,
    } = mock
    else {
        panic!("expected mock response transport fault");
    };
    assert_eq!(status.as_u16(), 202);
    assert_eq!(headers["x-mock"], "yes");
    assert_eq!(body, Bytes::from_static(br#"{"mock":true}"#));
}
