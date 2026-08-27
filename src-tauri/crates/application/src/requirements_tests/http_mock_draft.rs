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
        .await
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
    assert!(headers.contains(&("content-length".into(), body_bytes.len().to_string())));
    assert_eq!(ports.rule_validation_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.rule_save_calls.load(Ordering::SeqCst), 0);
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
        .await
        .unwrap();
    let RuleAction::Terminal {
        action: RuleTerminalAction::MockResponse { headers, .. },
    } = &draft.actions[0]
    else {
        panic!("mock response action expected");
    };
    assert_eq!(
        headers,
        &vec![
            ("x-public".into(), "keep-me".into()),
            ("content-length".into(), "2".into())
        ]
    );
}

#[tokio::test]
async fn compressed_non_utf8_and_incomplete_sources_are_rejected() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let compressed = record("HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n", "gzip");
    assert_eq!(
        application
            .rule_create_from_exchange_observation(&compressed, 2)
            .await
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
            .await
            .unwrap_err()
            .view_model
            .code,
        "HTTP_MOCK_DRAFT_BODY_NOT_UTF8"
    );

    binary.evidence_evicted = true;
    assert_eq!(
        application
            .rule_create_from_exchange_observation(&binary, 2)
            .await
            .unwrap_err()
            .view_model
            .code,
        "HTTP_MOCK_DRAFT_SOURCE_INVALID"
    );
}
