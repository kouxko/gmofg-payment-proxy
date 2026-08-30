use super::*;

fn http_context(header: &str, body: &str, body_is_utf8: bool) -> ExchangeContext {
    ExchangeContext::Http {
        header: header.into(),
        body: body.into(),
        body_is_utf8,
    }
}

fn record(response_header: &str, response_body: &str) -> ExchangeObservationRecord {
    ExchangeObservationRecord {
        exchange_id: "exchange-http-1".into(),
        workspace_id: WorkspaceId::new(),
        listener_id: ListenerId::new(),
        runtime_epoch: Uuid::new_v4(),
        peer_address: "127.0.0.1:12345".into(),
        protocol: ExchangeProtocol::Http,
        events: vec![
            ExchangeObservationEvent::Opened {
                observed_at: Utc::now(),
            },
            ExchangeObservationEvent::Sent {
                observed_at: Utc::now(),
                direction: ProtocolDirection::Upstream,
                context: http_context(
                    "POST /payments/42?view=full HTTP/1.1\r\nHost: server\r\n\r\n",
                    "",
                    true,
                ),
            },
            ExchangeObservationEvent::Received {
                observed_at: Utc::now(),
                direction: ProtocolDirection::Downstream,
                context: http_context(response_header, response_body, true),
                document: None,
                display: None,
            },
        ],
        evidence_evicted: false,
    }
}

#[tokio::test]
async fn complete_server_response_creates_valid_unsaved_disabled_mock_draft() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(Arc::clone(&ports));
    let record = record(
        "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nContent-Length: 999\r\n\r\n",
        "{\"approved\":true}",
    );

    let draft = application
        .rule_create_from_exchange_observation(&record, 2)
        .unwrap();

    assert_eq!(draft.rule_id, None);
    assert_eq!(draft.expected_revision, None);
    assert!(!draft.enabled);
    assert_eq!(draft.stage, Some(MessageStage::Request));
    assert_eq!(
        draft.channel.as_ref().unwrap().as_str(),
        record.listener_id.to_string()
    );
    assert_eq!(
        draft.conditions,
        vec![RuleCondition::Field {
            field: RuleMatchField::PathOrRequestType,
            operator: RuleMatchOperator::Equals {
                value: "/payments/42?view=full".into(),
            },
        }]
    );
    let RuleAction::Terminal {
        action:
            RuleTerminalAction::MockResponse {
                status,
                headers,
                body_bytes,
            },
    } = &draft.actions[0]
    else {
        panic!("mock response action expected");
    };
    assert_eq!(*status, 201);
    assert_eq!(body_bytes, b"{\"approved\":true}");
    assert_eq!(
        headers
            .iter()
            .filter(|(name, _)| name == "set-cookie")
            .count(),
        2
    );
    assert!(!headers.iter().any(|(name, _)| name == "content-length"));
    assert_eq!(ports.rule_validation_calls.load(Ordering::SeqCst), 0);
    assert_eq!(ports.rule_save_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn complete_server_response_creates_a_unified_unsaved_disabled_mock_draft() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let record = record(
        "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 999\r\n\r\n",
        "{\"approved\":true}",
    );

    let input = application
        .rule_definition_create_from_exchange_observation(&record, 2)
        .unwrap();

    assert_eq!(input.rule_id, None);
    assert_eq!(input.expected_revision, None);
    assert!(!input.draft.enabled);
    assert_eq!(input.draft.listener_id, record.listener_id);
    assert_eq!(input.draft.stage, RuleStage::ProxyToUpstream);
    let RuleContent::Http(content) = input.draft.content else {
        panic!("unified HTTP content expected");
    };
    assert!(matches!(
        content.condition,
        intercept_proxy_domain::ConditionTree::All(ref children)
            if matches!(children.as_slice(), [intercept_proxy_domain::ConditionTree::Leaf(intercept_proxy_domain::Condition::Http {
                field: intercept_proxy_domain::MatchField::PathOrRequestType, ..
            })])
    ));
    assert!(matches!(
        content.actions.as_slice(),
        [intercept_proxy_domain::UnifiedAction::Terminal(
            intercept_proxy_domain::TerminalAction::MockResponse { status: 201, .. }
        )]
    ));
}

#[tokio::test]
async fn server_content_length_is_not_copied_into_domain_valid_mock_headers() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let record = record(
        "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 999\r\nX-Request-Id: request-42\r\n\r\n",
        "{\"approved\":true}",
    );

    let draft = application
        .rule_create_from_exchange_observation(&record, 2)
        .unwrap();

    let RuleAction::Terminal {
        action:
            RuleTerminalAction::MockResponse {
                status,
                headers,
                body_bytes,
            },
    } = &draft.actions[0]
    else {
        panic!("mock response action expected");
    };
    let domain_draft = intercept_proxy_domain::RuleDraft {
        expected_revision: None,
        name: draft.name.clone(),
        description: draft.description.clone(),
        enabled: draft.enabled,
        priority: u32::try_from(draft.priority).unwrap(),
        created_order: 0,
        channel: draft.channel.clone(),
        stage: intercept_proxy_domain::MessageStage::Request,
        conditions: Vec::new(),
        actions: vec![intercept_proxy_domain::HttpAction::Terminal(
            intercept_proxy_domain::TerminalAction::MockResponse {
                status: *status,
                headers: headers.clone(),
                body_bytes: body_bytes.clone(),
            },
        )],
        one_shot: draft.one_shot,
    };
    intercept_proxy_domain::validate_rule_draft(&domain_draft)
        .expect("generated mock draft must satisfy Domain validation");
    assert_eq!(*status, 201);
    assert_eq!(body_bytes, b"{\"approved\":true}");
    assert_eq!(
        headers,
        &vec![
            ("content-type".into(), "application/json".into()),
            ("x-request-id".into(), "request-42".into()),
        ]
    );
}

#[tokio::test]
async fn mock_draft_filters_transport_headers_and_connection_nominated_headers() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let record = record(
        "HTTP/1.1 200 OK\r\nConnection: keep-alive, x-private-hop\r\nKeep-Alive: timeout=5\r\nPrOxY-CoNnEcTiOn: keep-alive\r\nTransfer-Encoding: chunked\r\nX-Private-Hop: remove-me\r\nX-Public: keep-me\r\n\r\n",
        "ok",
    );
    let draft = application
        .rule_create_from_exchange_observation(&record, 2)
        .unwrap();
    let RuleAction::Terminal {
        action: RuleTerminalAction::MockResponse { headers, .. },
    } = &draft.actions[0]
    else {
        panic!("mock response action expected");
    };
    assert_eq!(headers, &vec![("x-public".into(), "keep-me".into())]);
}

#[tokio::test]
async fn compressed_non_utf8_and_incomplete_sources_are_rejected() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let compressed = record("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n", "gzip");
    assert_eq!(
        application
            .rule_create_from_exchange_observation(&compressed, 2)
            .unwrap_err()
            .view_model
            .code,
        "HTTP_MOCK_DRAFT_BODY_ENCODED"
    );

    let mut binary = record("HTTP/1.1 200 OK\r\n\r\n", "�");
    let ExchangeObservationEvent::Received {
        context: ExchangeContext::Http { body_is_utf8, .. },
        ..
    } = &mut binary.events[2]
    else {
        panic!("HTTP response expected");
    };
    *body_is_utf8 = false;
    assert_eq!(
        application
            .rule_create_from_exchange_observation(&binary, 2)
            .unwrap_err()
            .view_model
            .code,
        "HTTP_MOCK_DRAFT_BODY_NOT_UTF8"
    );

    binary.evidence_evicted = true;
    assert_eq!(
        application
            .rule_create_from_exchange_observation(&binary, 2)
            .unwrap_err()
            .view_model
            .code,
        "HTTP_MOCK_DRAFT_SOURCE_INVALID"
    );
}
